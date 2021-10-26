/// Mostly taken from flate2
///
/// Copyright (c) 2014 Alex Crichton
///
/// Permission is hereby granted, free of charge, to any
/// person obtaining a copy of this software and associated
/// documentation files (the "Software"), to deal in the
/// Software without restriction, including without
/// limitation the rights to use, copy, modify, merge,
/// publish, distribute, sublicense, and/or sell copies of
/// the Software, and to permit persons to whom the Software
/// is furnished to do so, subject to the following
/// conditions:
///
/// The above copyright notice and this permission notice
/// shall be included in all copies or substantial portions
/// of the Software.
///
/// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
/// ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
/// TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
/// PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
/// SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
/// CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
/// OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
/// IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
/// DEALINGS IN THE SOFTWARE.
use libc::{c_int, c_uint, c_void};
use libz_sys as ffi;
use log::{error, trace};

use std::{
    alloc::{self, Layout},
    mem, ptr, slice,
};

const ZLIB_VERSION: &str = "1.2.8\0";

type AllocSize = ffi::uInt;
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Compressing,
}

const ALIGN: usize = std::mem::align_of::<usize>();

fn align_up(size: usize, align: usize) -> usize {
    (size + align - 1) & !(align - 1)
}

extern "C" fn zalloc(_ptr: *mut c_void, items: AllocSize, item_size: AllocSize) -> *mut c_void {
    // We need to multiply `items` and `item_size` to get the actual desired
    // allocation size. Since `zfree` doesn't receive a size argument we
    // also need to allocate space for a `usize` as a header so we can store
    // how large the allocation is to deallocate later.
    let size = match items
        .checked_mul(item_size)
        .and_then(|i| usize::try_from(i).ok())
        .map(|size| align_up(size, ALIGN))
        .and_then(|i| i.checked_add(std::mem::size_of::<usize>()))
    {
        Some(i) => i,
        None => return ptr::null_mut(),
    };

    // Make sure the `size` isn't too big to fail `Layout`'s restrictions
    let layout = match Layout::from_size_align(size, ALIGN) {
        Ok(layout) => layout,
        Err(_) => return ptr::null_mut(),
    };

    unsafe {
        // Allocate the data, and if successful store the size we allocated
        // at the beginning and then return an offset pointer.
        let ptr = alloc::alloc(layout).cast::<usize>();
        if ptr.is_null() {
            return ptr.cast::<libc::c_void>();
        }
        *ptr = size;
        ptr.add(1).cast::<libc::c_void>()
    }
}

extern "C" fn zfree(_ptr: *mut c_void, address: *mut c_void) {
    unsafe {
        // Move our address being free'd back one pointer, read the size we
        // stored in `zalloc`, and then free it using the standard Rust
        // allocator.
        let ptr = (address.cast::<usize>()).offset(-1);
        let size = *ptr;
        let layout = Layout::from_size_align_unchecked(size, ALIGN);
        alloc::dealloc(ptr.cast::<u8>(), layout);
    }
}

trait Context {
    fn stream(&mut self) -> &mut ffi::z_stream;

    fn stream_apply<F>(&mut self, input: &[u8], output: &mut Vec<u8>, each: F) -> Result<()>
    where
        F: Fn(&mut ffi::z_stream) -> Option<Result<()>>,
    {
        debug_assert!(output.is_empty(), "Output vector is not empty.");

        let stream = self.stream();

        stream.next_in = input.as_ptr() as *mut _;
        stream.avail_in = input.len() as c_uint;

        let mut output_size;

        loop {
            output_size = output.len();

            if output_size == output.capacity() {
                output.reserve(input.len());
            }

            let out_slice = unsafe {
                slice::from_raw_parts_mut(
                    output.as_mut_ptr().add(output_size),
                    output.capacity() - output_size,
                )
            };

            stream.next_out = out_slice.as_mut_ptr();
            stream.avail_out = out_slice.len() as c_uint;

            let before = stream.total_out;
            let cont = each(stream);

            unsafe {
                output.set_len((stream.total_out - before) as usize + output_size);
            }

            if let Some(result) = cont {
                return result;
            }
        }
    }
}

pub struct Compressor {
    // Box the z_stream to ensure it isn't moved. Moving the z_stream
    // causes zlib to fail, because it maintains internal pointers.
    stream: Box<ffi::z_stream>,
}

unsafe impl Send for Compressor {}
unsafe impl Sync for Compressor {}

impl Compressor {
    pub fn new(window_bits: i8) -> Compressor {
        debug_assert!(window_bits >= 9, "Received too small window size.");
        debug_assert!(window_bits <= 15, "Received too large window size.");

        unsafe {
            let mut stream: Box<ffi::z_stream> = Box::new(ffi::z_stream {
                next_in: ptr::null_mut(),
                avail_in: 0,
                total_in: 0,
                next_out: ptr::null_mut(),
                avail_out: 0,
                total_out: 0,
                msg: ptr::null_mut(),
                adler: 0,
                data_type: 0,
                reserved: 0,
                opaque: ptr::null_mut(),
                state: ptr::null_mut(),
                zalloc,
                zfree,
            });

            let result = ffi::deflateInit2_(
                stream.as_mut(),
                9,
                ffi::Z_DEFLATED,
                // - if no header
                c_int::from(window_bits),
                9,
                ffi::Z_DEFAULT_STRATEGY,
                ZLIB_VERSION.as_ptr().cast::<i8>(),
                mem::size_of::<ffi::z_stream>() as c_int,
            );
            assert!(result == ffi::Z_OK, "Failed to initialize compresser.");
            Compressor { stream }
        }
    }

    pub fn compress(&mut self, input: &[u8], output: &mut Vec<u8>) -> Result<()> {
        self.stream_apply(input, output, |stream| unsafe {
            match ffi::deflate(stream, ffi::Z_SYNC_FLUSH) {
                ffi::Z_OK | ffi::Z_BUF_ERROR => {
                    if stream.avail_in == 0 && stream.avail_out > 0 {
                        Some(Ok(()))
                    } else {
                        None
                    }
                }
                _ => Some(Err(Error::Compressing)),
            }
        })
    }
}

impl Context for Compressor {
    fn stream(&mut self) -> &mut ffi::z_stream {
        self.stream.as_mut()
    }
}

impl Drop for Compressor {
    fn drop(&mut self) {
        match unsafe { ffi::deflateEnd(self.stream.as_mut()) } {
            ffi::Z_STREAM_ERROR => error!("Compression stream encountered bad state."),
            // Ignore discarded data error because we are raw
            ffi::Z_OK | ffi::Z_DATA_ERROR => trace!("Deallocated compression context."),
            code => error!("Bad zlib status encountered: {}", code),
        }
    }
}

#[test]
fn test_compress() {
    let bytes: &[u8] =
        b"{\"t\": null, \"s\": null, \"op\": 10, \"d\": {\"heartbeat_interval\": 41250}}";
    let mut output = Vec::with_capacity(bytes.len());
    let mut encoder = Compressor::new(15);
    assert!(encoder.compress(&bytes, &mut output).is_ok());
    assert!(output.len() > 5);
    output.clear();
    assert!(encoder.compress(&bytes, &mut output).is_ok());
    assert!(output.len() > 5);
}
