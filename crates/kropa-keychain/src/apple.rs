// SPDX-License-Identifier: GPL-3.0-or-later

//! What only macOS can tell: the audit token of the peer of a socket (`LOCAL_PEERTOKEN`) and the
//! code signature of a process or a file (`Security.framework`, code signing services).
//!
//! The wrappers of the `security-framework` crate are used wherever they exist (`SecCode`,
//! `SecRequirement`, guest attributes). `unsafe` is allowed in this module only, for what they do
//! not cover: `getsockopt` with `LOCAL_PEERTOKEN`, and three functions of the code signing services
//! to read the designated requirement and the signing flags of a code object. Each block says why it
//! holds.
//!
//! **Nothing in this module has run on a Mac when it was written**: it type-checks for
//! `aarch64-apple-darwin` and the macOS CI exercises what can run without a certificate; what needs
//! one (a peer signed with it, the hardened runtime of a real `kropa-app`) is the manual list of
//! `docs/testing/manual-macos.md`. Every check here fails closed.
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::mem::size_of;
use std::os::fd::RawFd;
use std::path::Path;
use std::ptr;
use std::str::FromStr;

use core_foundation::base::{CFType, CFTypeRef, TCFType};
use core_foundation::data::CFData;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringRef};
use core_foundation::url::CFURL;
use security_framework::os::macos::code_signing::{
    Flags, GuestAttributes, SecCode, SecRequirement, SecStaticCode,
};

use crate::guard::{
    PeerToken, ProjectCertificate, Unverified, broker_requirement, certificate_from_requirement,
    client_requirement,
};
use crate::system::int_option;

/// `kSecCSSigningInformation` of `SecCodeCopySigningInformation`.
const SIGNING_INFORMATION: u32 = 1 << 1;

/// `kSecCodeSignatureRuntime` (`CS_RUNTIME`): the hardened runtime flag of a signature.
const FLAG_RUNTIME: i64 = 0x1_0000;

#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    fn SecCodeCopyDesignatedRequirement(
        code: *const c_void,
        flags: u32,
        requirement: *mut CFTypeRef,
    ) -> i32;
    fn SecRequirementCopyString(requirement: CFTypeRef, flags: u32, text: *mut CFStringRef) -> i32;
    fn SecCodeCopySigningInformation(
        code: *const c_void,
        flags: u32,
        information: *mut CFDictionaryRef,
    ) -> i32;
    static kSecCodeInfoFlags: CFStringRef;
}

/// The audit token of the process that made the other end of the socket `fd`.
///
/// `LOCAL_PEERTOKEN` (`sys/un.h`, macOS 10.15 and later) is checked against `LOCAL_PEERPID`: both
/// must name the same process.
pub(crate) fn peer_token(fd: RawFd) -> Result<PeerToken, Unverified> {
    let mut raw = [0_u32; 8];
    let size = size_of::<[u32; 8]>();
    let Ok(mut len) = libc::socklen_t::try_from(size) else {
        return Err(Unverified("audit token size"));
    };
    // SAFETY: `raw` is valid for writes of `len` bytes, `len` is its size; `fd` is only a number (a
    // bad one fails with EBADF).
    let rc = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_LOCAL,
            libc::LOCAL_PEERTOKEN,
            raw.as_mut_ptr().cast::<c_void>(),
            &raw mut len,
        )
    };
    if rc != 0 || len as usize != size {
        return Err(Unverified("LOCAL_PEERTOKEN could not be read"));
    }
    // `audit_token_t` is `unsigned int val[8]`; `audit_token_to_pid` is `val[5]`.
    let pid = raw[5];
    let by_pid = int_option(fd, libc::SOL_LOCAL, libc::LOCAL_PEERPID)
        .ok()
        .and_then(|p| u32::try_from(p).ok());
    if by_pid != Some(pid) {
        return Err(Unverified("LOCAL_PEERTOKEN and LOCAL_PEERPID disagree"));
    }
    Ok(PeerToken { pid, raw })
}

/// The text of the designated requirement of a code object.
fn designated_requirement(code: &SecCode) -> Result<String, Unverified> {
    let mut requirement: CFTypeRef = ptr::null();
    // SAFETY: `code` is a live SecCode (a SecCode may stand for a static code object); the out
    // pointer is valid for a write. On success the requirement is a new reference we own.
    let status = unsafe {
        SecCodeCopyDesignatedRequirement(
            code.as_concrete_TypeRef().cast::<c_void>(),
            0,
            &mut requirement,
        )
    };
    if status != 0 || requirement.is_null() {
        return Err(Unverified("the designated requirement cannot be read"));
    }
    // SAFETY: the call above returned an owned reference (Copy rule), released when this drops.
    let requirement = unsafe { CFType::wrap_under_create_rule(requirement) };
    let mut text: CFStringRef = ptr::null();
    // SAFETY: `requirement` is a live SecRequirement; the out pointer is valid for a write. On
    // success the string is a new reference we own.
    let status = unsafe { SecRequirementCopyString(requirement.as_CFTypeRef(), 0, &mut text) };
    if status != 0 || text.is_null() {
        return Err(Unverified("the designated requirement cannot be printed"));
    }
    // SAFETY: owned reference, as above.
    let text = unsafe { CFString::wrap_under_create_rule(text) };
    Ok(text.to_string())
}

/// Whether the signature of `code` carries the hardened runtime flag.
fn has_hardened_runtime(code: &SecCode) -> Result<bool, Unverified> {
    let mut information: CFDictionaryRef = ptr::null();
    // SAFETY: `code` is a live SecCode; the out pointer is valid for a write. On success the
    // dictionary is a new reference we own.
    let status = unsafe {
        SecCodeCopySigningInformation(
            code.as_concrete_TypeRef().cast::<c_void>(),
            SIGNING_INFORMATION,
            &mut information,
        )
    };
    if status != 0 || information.is_null() {
        return Err(Unverified("the signing flags cannot be read"));
    }
    // SAFETY: owned reference, released when it drops.
    let information =
        unsafe { CFDictionary::<CFString, CFNumber>::wrap_under_create_rule(information) };
    // SAFETY: `kSecCodeInfoFlags` is a constant CFString of the framework, valid for the whole
    // process, read under the get rule (retained, not owned).
    let key = unsafe { CFString::wrap_under_get_rule(kSecCodeInfoFlags) };
    let flags = information
        .find(&key)
        .and_then(|number| number.to_i64())
        .ok_or(Unverified("the signing flags are not in the signature"))?;
    Ok(flags & FLAG_RUNTIME != 0)
}

/// The leaf certificate of the project, from the broker's own signature. An ad hoc signature (the
/// designated requirement is a `cdhash`), no signature, or a signature that is not a plain
/// certificate leaf, gives none: the broker serves nobody then.
pub(crate) fn own_certificate() -> Result<ProjectCertificate, Unverified> {
    let me = SecCode::for_self(Flags::NONE)
        .map_err(|_| Unverified("the broker cannot read its own signature"))?;
    let text = designated_requirement(&me)?;
    certificate_from_requirement(&text).ok_or(Unverified(
        "not signed with a certificate of the project (ad hoc, unsigned or other)",
    ))
}

fn parse_requirement(text: &str) -> Result<SecRequirement, Unverified> {
    SecRequirement::from_str(text).map_err(|_| Unverified("the code requirement is not accepted"))
}

/// Whether the process behind `token` is `kropa-app`: signed with `certificate`, under the
/// identifier of `kropa-app`, strictly valid, with the hardened runtime.
pub(crate) fn check_peer(
    token: &PeerToken,
    certificate: &ProjectCertificate,
) -> Result<(), Unverified> {
    let requirement = parse_requirement(&client_requirement(certificate))?;
    let bytes: Vec<u8> = token.raw.iter().flat_map(|w| w.to_ne_bytes()).collect();
    let data = CFData::from_buffer(&bytes);
    let mut attributes = GuestAttributes::new();
    attributes.set_audit_token(data.as_concrete_TypeRef());
    // The token carries the version of the pid: a process that was replaced (`exec`, pid reused)
    // since the pair was made does not answer to it.
    let code = SecCode::copy_guest_with_attribues(None, &attributes, Flags::NONE)
        .map_err(|_| Unverified("no process answers to the audit token of the peer"))?;
    code.check_validity(Flags::STRICT_VALIDATE, &requirement)
        .map_err(|_| Unverified("the code of the peer does not satisfy the requirement"))?;
    if !has_hardened_runtime(&code)? {
        return Err(Unverified(
            "the peer is not signed with the hardened runtime",
        ));
    }
    Ok(())
}

/// Whether the file at `path` is the project's broker: signed with the same certificate as this
/// program, under the identifier of `kropa`, strictly valid. Called by the client before it starts
/// the broker.
pub(crate) fn verify_broker_binary(path: &Path) -> Result<(), Unverified> {
    let certificate = own_certificate()?;
    let requirement = parse_requirement(&broker_requirement(&certificate))?;
    let url = CFURL::from_path(path, false).ok_or(Unverified("the broker path cannot be used"))?;
    let code = SecStaticCode::from_path(&url, Flags::NONE)
        .map_err(|_| Unverified("the broker file is not signed code"))?;
    code.check_validity(Flags::STRICT_VALIDATE, &requirement)
        .map_err(|_| Unverified("the broker file does not satisfy the requirement of kropa"))
}

#[cfg(test)]
mod tests {
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixStream;

    use super::{
        designated_requirement, has_hardened_runtime, own_certificate, parse_requirement,
        peer_token,
    };
    use crate::guard::{
        ProjectCertificate, broker_requirement, certificate_from_requirement, client_requirement,
    };
    use security_framework::os::macos::code_signing::{Flags, SecCode};

    #[test]
    fn the_peer_token_of_a_socket_pair_names_this_process() {
        // Section 2.2.1 point 7, first item: LOCAL_PEERTOKEN on a socketpair gives the creator.
        let (a, _b) = UnixStream::pair().unwrap();
        let token = peer_token(a.as_raw_fd()).expect("LOCAL_PEERTOKEN on a socket pair");
        assert_eq!(token.pid, std::process::id());
    }

    #[test]
    fn the_peer_token_of_something_else_is_refused() {
        let file = std::fs::File::open("/dev/null").unwrap();
        assert!(peer_token(file.as_raw_fd()).is_err());
    }

    #[test]
    fn the_requirements_we_build_are_accepted_by_the_code_signing_services() {
        let certificate = ProjectCertificate::from_hex(&"ab".repeat(20)).unwrap();
        parse_requirement(&client_requirement(&certificate)).expect("client requirement");
        parse_requirement(&broker_requirement(&certificate)).expect("broker requirement");
    }

    #[test]
    fn the_guest_of_our_own_audit_token_is_this_very_process() {
        use core_foundation::base::TCFType;
        use core_foundation::data::CFData;
        use security_framework::os::macos::code_signing::GuestAttributes;

        let (a, _b) = UnixStream::pair().unwrap();
        let token = peer_token(a.as_raw_fd()).unwrap();
        let bytes: Vec<u8> = token.raw.iter().flat_map(|w| w.to_ne_bytes()).collect();
        let data = CFData::from_buffer(&bytes);
        let mut attributes = GuestAttributes::new();
        attributes.set_audit_token(data.as_concrete_TypeRef());
        let guest = SecCode::copy_guest_with_attribues(None, &attributes, Flags::NONE)
            .expect("a guest for the token of this process");
        let path = guest.path(Flags::NONE).unwrap().to_path().unwrap();
        assert_eq!(path, std::env::current_exe().unwrap());
    }

    #[test]
    fn a_test_binary_has_no_project_certificate_so_the_broker_would_serve_nobody() {
        // `cargo` links ad hoc ("linker-signed"): the designated requirement is a cdhash, or the
        // binary is not signed at all. Either way there is no certificate to pin.
        let me = SecCode::for_self(Flags::NONE).unwrap();
        if let Ok(text) = designated_requirement(&me) {
            assert!(certificate_from_requirement(&text).is_none(), "{text}");
        }
        assert!(own_certificate().is_err());
    }

    #[test]
    fn a_test_binary_is_not_signed_with_the_hardened_runtime() {
        let me = SecCode::for_self(Flags::NONE).unwrap();
        // The flags are readable for a linker-signed binary, and the runtime flag is absent; if the
        // read itself fails the check fails closed, which is also a refusal.
        if let Ok(runtime) = has_hardened_runtime(&me) {
            assert!(!runtime);
        }
    }
}
