//! TCC permission state, prompts, and a way out when the prompt will never
//! appear again.
//!
//! Four separate grants are involved and they fail in different ways: without
//! Accessibility nothing can be injected, without Input Monitoring the hotkey
//! silently never fires, without the microphone recording yields silence, and
//! without Screen Recording the OCR context is empty. The Swift app checked
//! exactly one of them.

use objc2_app_kit::NSWorkspace;
use objc2_application_services::{kAXTrustedCheckOptionPrompt, AXIsProcessTrustedWithOptions};
use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};
use objc2_core_foundation::CFDictionary;
use objc2_core_graphics::{
    CGPreflightListenEventAccess, CGPreflightScreenCaptureAccess, CGRequestListenEventAccess,
    CGRequestScreenCaptureAccess,
};
use objc2_foundation::{NSDictionary, NSNumber, NSString, NSURL};
use tracing::{debug, warn};

use crate::{Permission, PermissionState, Permissions};

pub struct MacPermissions;

impl Permissions for MacPermissions {
    fn status(&self, permission: Permission) -> PermissionState {
        match permission {
            Permission::Accessibility => granted(is_trusted(false)),
            // Preflight never prompts; it only reports.
            Permission::InputMonitoring => granted(CGPreflightListenEventAccess()),
            Permission::ScreenRecording => granted(CGPreflightScreenCaptureAccess()),
            Permission::Microphone => {
                let Some(audio) = audio_media_type() else {
                    return PermissionState::NotApplicable;
                };
                // SAFETY: `audio` is one of the two media types this call
                // accepts; anything else raises an Objective-C exception.
                let status = unsafe { AVCaptureDevice::authorizationStatusForMediaType(audio) };
                match status {
                    AVAuthorizationStatus::Authorized => PermissionState::Granted,
                    AVAuthorizationStatus::NotDetermined => PermissionState::NotDetermined,
                    // Restricted means an MDM profile forbids it. There is no
                    // user action that changes that, so it reads as denied.
                    _ => PermissionState::Denied,
                }
            }
        }
    }

    fn request(&self, permission: Permission) {
        match permission {
            Permission::Accessibility => {
                // Prompting is asynchronous and does not affect the return
                // value, so the result here is uninteresting.
                is_trusted(true);
            }
            Permission::InputMonitoring => {
                CGRequestListenEventAccess();
            }
            Permission::ScreenRecording => {
                CGRequestScreenCaptureAccess();
            }
            Permission::Microphone => {
                let Some(audio) = audio_media_type() else {
                    return;
                };
                let handler = block2::RcBlock::new(|granted: objc2::runtime::Bool| {
                    debug!(granted = granted.as_bool(), "microphone access decided");
                });
                // SAFETY: `audio` is accepted by this call, and AVFoundation
                // retains the block for as long as the prompt is up.
                unsafe {
                    AVCaptureDevice::requestAccessForMediaType_completionHandler(audio, &handler);
                }
            }
        }
    }

    fn open_settings(&self, permission: Permission) {
        // Once a grant has been denied macOS never prompts again, so the only
        // remaining path is the Privacy pane itself.
        let pane = match permission {
            Permission::Accessibility => "Privacy_Accessibility",
            Permission::InputMonitoring => "Privacy_ListenEvent",
            Permission::ScreenRecording => "Privacy_ScreenCapture",
            Permission::Microphone => "Privacy_Microphone",
        };
        let url = format!("x-apple.systempreferences:com.apple.preference.security?{pane}");
        match NSURL::URLWithString(&NSString::from_str(&url)) {
            Some(url) => {
                NSWorkspace::sharedWorkspace().openURL(&url);
            }
            None => warn!(%url, "could not build the settings URL"),
        }
    }
}

fn granted(value: bool) -> PermissionState {
    if value {
        PermissionState::Granted
    } else {
        // TCC does not distinguish "never asked" from "refused" for these
        // three, and the remedy is the same either way.
        PermissionState::Denied
    }
}

/// `AXIsProcessTrustedWithOptions`, optionally raising the system prompt.
fn is_trusted(prompt: bool) -> bool {
    if !prompt {
        // SAFETY: a null options dictionary is explicitly supported and means
        // "check without prompting".
        return unsafe { AXIsProcessTrustedWithOptions(None) };
    }

    // SAFETY: the key is an immortal framework constant, and `CFString` and
    // `NSString` are toll-free bridged so it is a valid dictionary key here.
    let key = unsafe { &*(kAXTrustedCheckOptionPrompt as *const _ as *const NSString) };
    let options = NSDictionary::from_slices(&[key], &[&*NSNumber::new_bool(true)]);
    // SAFETY: `NSDictionary` and `CFDictionary` are toll-free bridged, and the
    // dictionary holds the one key this call documents with a boolean value.
    let options = unsafe { &*(objc2::rc::Retained::as_ptr(&options) as *const CFDictionary) };
    // SAFETY: the options dictionary has the documented key and value types.
    unsafe { AXIsProcessTrustedWithOptions(Some(options)) }
}

/// The `AVMediaTypeAudio` constant.
///
/// AVFoundation declares it nullable, and passing anything other than the
/// audio or video constant raises an Objective-C exception rather than
/// returning an error, so the check is not ceremony.
fn audio_media_type() -> Option<&'static objc2_av_foundation::AVMediaType> {
    // SAFETY: reading an immortal framework string constant.
    unsafe { AVMediaTypeAudio }
}
