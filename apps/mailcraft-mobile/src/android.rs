//! The Kotlin shell's native methods: `io.github.zidan44.mailcraft.Core`'s `external fun`s.
//!
//! Each one only moves text across and calls the crate's own function; an error comes back as
//! its text (empty when all went well), never as an exception, and `with_env` keeps a panic from
//! crossing into the JVM.
//!
//! The one `unsafe` here is `#[unsafe(no_mangle)]`: the JVM finds a native method by its
//! exported name (`Java_<package>_<class>_<method>`), so these names must stay as they are.
//! SAFETY (each): the signature is the JNI one for a static native method of these Java types,
//! the name is exported once (no other symbol has it), and the body touches no raw pointer.
#![allow(unsafe_code)]

use jni::EnvUnowned;
use jni::errors::ThrowRuntimeExAndDefault;
use jni::objects::{JClass, JString};
use jni::sys::{jint, jlong};

/// The text of a Java string; an unreadable one is empty (what a call then refuses).
fn text(s: &JString<'_>) -> String {
    s.to_string()
}

/// `Ok` as empty, an error as its text: what the Kotlin side shows.
fn said(r: Result<(), String>) -> String {
    r.err().unwrap_or_default()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_zidan44_mailcraft_Core_start<'c>(
    mut env: EnvUnowned<'c>,
    _class: JClass<'c>,
    dir: JString<'c>,
    secrets: JString<'c>,
    offset: jint,
    zone: JString<'c>,
) -> JString<'c> {
    env.with_env(|env| -> Result<_, jni::errors::Error> {
        let r = said(crate::start(&text(&dir), &text(&secrets), offset, &text(&zone)));
        JString::from_str(env, r)
    })
    .resolve::<ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_zidan44_mailcraft_Core_send<'c>(
    mut env: EnvUnowned<'c>,
    _class: JClass<'c>,
    action: JString<'c>,
) -> JString<'c> {
    env.with_env(|env| -> Result<_, jni::errors::Error> {
        let r = said(crate::send(&text(&action)));
        JString::from_str(env, r)
    })
    .resolve::<ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_zidan44_mailcraft_Core_poll<'c>(mut env: EnvUnowned<'c>, _class: JClass<'c>, timeout_ms: jlong) -> JString<'c> {
    env.with_env(|env| -> Result<_, jni::errors::Error> {
        let m = crate::poll(u64::try_from(timeout_ms).unwrap_or(0));
        JString::from_str(env, m)
    })
    .resolve::<ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_zidan44_mailcraft_Core_view<'c>(mut env: EnvUnowned<'c>, _class: JClass<'c>) -> JString<'c> {
    env.with_env(|env| -> Result<_, jni::errors::Error> { JString::from_str(env, crate::view()) }).resolve::<ThrowRuntimeExAndDefault>()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_zidan44_mailcraft_Core_zone<'c>(
    mut env: EnvUnowned<'c>,
    _class: JClass<'c>,
    offset: jint,
    zone: JString<'c>,
) -> JString<'c> {
    env.with_env(|env| -> Result<_, jni::errors::Error> {
        let r = said(crate::zone(offset, &text(&zone)));
        JString::from_str(env, r)
    })
    .resolve::<ThrowRuntimeExAndDefault>()
}
