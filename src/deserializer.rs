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
use std::{ops::Range, str::FromStr};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GatewayEvent<'a> {
    event_type: Option<EventTypeInfo<'a>>,
    op: OpInfo,
    sequence: Option<SequenceInfo>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpInfo(pub u8, pub Range<usize>);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventTypeInfo<'a>(pub &'a str, pub Range<usize>);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SequenceInfo(pub u64, pub Range<usize>);

impl<'a> GatewayEvent<'a> {
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

    /// Return the opcode of the payload.
    pub const fn op(&self) -> u8 {
        self.op.0
    }

    /// Consume the deserializer, returning its opcode and event type
    /// components.
    pub const fn into_parts(self) -> (OpInfo, Option<SequenceInfo>, Option<EventTypeInfo<'a>>) {
        (self.op, self.sequence, self.event_type)
    }

    fn find_event_type(input: &'a str) -> Option<EventTypeInfo<'a>> {
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
        let range = start..start + to;

        input
            .get(range.clone())
            .map(|event_type| EventTypeInfo(event_type, range))
    }

    fn find_opcode(input: &'a str) -> Option<OpInfo> {
        Self::find_integer(input, r#""op":"#).map(|(op, pos)| OpInfo(op, pos))
    }

    fn find_sequence(input: &'a str) -> Option<SequenceInfo> {
        Self::find_integer(input, r#""s":"#).map(|(seq, pos)| SequenceInfo(seq, pos))
    }

    fn find_integer<T: FromStr>(input: &'a str, key: &str) -> Option<(T, Range<usize>)> {
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
        let range = from..from + to;
        let clean = input.get(range.clone())?;

        T::from_str(clean).ok().map(|int| (int, range))
    }
}
