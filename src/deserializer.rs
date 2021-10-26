/// This file is modified from Twilight to also include the position of each
///
/// ISC License (ISC)
///
/// Copyright (c) 2019 (c) The Twilight Contributors
///
/// Permission to use, copy, modify, and/or distribute this software for any purpose
/// with or without fee is hereby granted, provided that the above copyright notice
/// and this permission notice appear in all copies.
///
/// THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH
/// REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY
/// AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT,
/// INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM LOSS
/// OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR OTHER
/// TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE
/// OF THIS SOFTWARE.
use std::str::FromStr;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GatewayEventDeserializer<'a> {
    event_type: Option<(&'a str, usize)>,
    op: (u8, usize),
    sequence: Option<(u64, usize)>,
}

impl<'a> GatewayEventDeserializer<'a> {
    /// Create a gateway event deserializer with some information found by
    /// scanning the JSON payload to deserialise.
    ///
    /// This will scan the payload for the opcode and, optionally, event type if
    /// provided. The opcode key ("op"), must be in the payload while the event
    /// type key ("t") is optional and only required for event ops.
    pub fn from_json(input: &'a str) -> Option<Self> {
        let op = Self::find_opcode(input)?;
        let event_type = Self::find_event_type(input);
        let sequence = Self::find_sequence(input);

        Some(Self {
            event_type,
            op,
            sequence,
        })
    }

    /// Return an immutable reference to the event type of the payload.
    pub fn event_type_ref(&self) -> Option<&str> {
        self.event_type.map(|v| v.0)
    }

    /// Return the opcode of the payload.
    pub const fn op(&self) -> u8 {
        self.op.0
    }

    /// Return the sequence of the payload.
    pub fn sequence(&self) -> Option<u64> {
        self.sequence.map(|v| v.0)
    }

    /// Consume the deserializer, returning its opcode and event type
    /// components.
    pub const fn into_parts(self) -> ((u8, usize), Option<(u64, usize)>, Option<(&'a str, usize)>) {
        (self.op, self.sequence, self.event_type)
    }

    fn find_event_type(input: &'a str) -> Option<(&'a str, usize)> {
        // We're going to search for the event type key from the start. Discord
        // always puts it at the front before the D key from some testing of
        // several hundred payloads.
        //
        // If we find it, add 4, since that's the length of what we're searching
        // for.
        let from = input.find(r#""t":"#)? + 4;

        // Now let's find where the value starts, which may be a string or null.
        // Or maybe something else. If it's anything but a string, then there's
        // no event type.
        let start = input.get(from..)?.find(|c: char| !c.is_whitespace())? + from + 1;

        // Check if the character just before the cursor is '"'.
        if input.as_bytes().get(start - 1).copied()? != b'"' {
            return None;
        }

        let to = input.get(start..)?.find('"')?;

        input
            .get(start..start + to)
            .map(|event_type| (event_type, start))
    }

    fn find_opcode(input: &'a str) -> Option<(u8, usize)> {
        Self::find_integer(input, r#""op":"#)
    }

    fn find_sequence(input: &'a str) -> Option<(u64, usize)> {
        Self::find_integer(input, r#""s":"#)
    }

    fn find_integer<T: FromStr>(input: &'a str, key: &str) -> Option<(T, usize)> {
        // Find the op key's position and then search for where the first
        // character that's not base 10 is. This'll give us the bytes with the
        // op which can be parsed.
        //
        // Add 5 at the end since that's the length of what we're finding.
        let from = input.find(key)? + key.len();

        // Look for the first thing that isn't a base 10 digit or whitespace,
        // i.e. a comma (denoting another JSON field), curly brace (end of the
        // object), etc. This'll give us the op number, maybe with a little
        // whitespace.
        let to = input.get(from..)?.find(&[',', '}'] as &[_])?;
        // We might have some whitespace, so let's trim this.
        let clean = input.get(from..from + to)?.trim();

        T::from_str(clean).ok().map(|int| (int, from))
    }
}
