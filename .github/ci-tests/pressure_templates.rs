pub const CAUSAL: &str = r###"#![no_std]
#![deny(warnings)]

use hibana::g;
use hibana::runtime::program::{project, RoleProgram};

#[inline(never)]
pub fn projected_roles() -> (RoleProgram<0>, RoleProgram<1>, RoleProgram<2>) {
    let program = {PROGRAM};
    (project(&program), project(&program), project(&program))
}
"###;
pub const ROUTE: &str = r###"#![no_std]
#![deny(warnings)]

use hibana::g;
use hibana::runtime::program::{project, RoleProgram};
use hibana::runtime::wire::{CodecError, Payload as WirePayloadView, WireEncode, WirePayload};

#[derive(Clone, Copy)]
struct Payload<const ID: u16>;

impl<const ID: u16> WireEncode for Payload<ID> {
    fn encode_into(&self, _out: &mut [u8]) -> Result<usize, CodecError> { Ok(0) }
}

impl<const ID: u16> WirePayload for Payload<ID> {
    const SCHEMA_ID: u32 = 0x5001_0000 | ID as u32;
    type Decoded<'a> = Self;
    fn validate_payload(input: WirePayloadView<'_>) -> Result<(), CodecError> {
        if input.as_bytes().is_empty() { Ok(()) } else { Err(CodecError::Malformed) }
    }
    fn decode_validated_payload<'a>(input: WirePayloadView<'a>) -> Self::Decoded<'a> {
        let _ = input.as_bytes();
        Self
    }
}

#[inline(never)]
pub fn projected_pair() -> (RoleProgram<0>, RoleProgram<1>) {
    let program = {PROGRAM};
    (project(&program), project(&program))
}
"###;
pub const MESSAGE: &str = r###"#![no_std]
#![deny(warnings)]

use hibana::g;
use hibana::runtime::program::{project, RoleProgram};
use hibana::runtime::wire::{CodecError, Payload as WirePayloadView, WireEncode, WirePayload};

#[derive(Clone, Copy)]
struct Payload<const ID: u16>;

impl<const ID: u16> WireEncode for Payload<ID> {
    fn encode_into(&self, _out: &mut [u8]) -> Result<usize, CodecError> { Ok(0) }
}

impl<const ID: u16> WirePayload for Payload<ID> {
    const SCHEMA_ID: u32 = 0x4001_0000 | ID as u32;
    type Decoded<'a> = Self;
    fn validate_payload(input: WirePayloadView<'_>) -> Result<(), CodecError> {
        if input.as_bytes().is_empty() { Ok(()) } else { Err(CodecError::Malformed) }
    }
    fn decode_validated_payload<'a>(input: WirePayloadView<'a>) -> Self::Decoded<'a> {
        let _ = input.as_bytes();
        Self
    }
}

#[inline(never)]
pub fn projected_pair() -> (RoleProgram<0>, RoleProgram<1>) {
    let program = {PROGRAM};
    (project(&program), project(&program))
}
"###;
