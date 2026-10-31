use agent_desktop_core::{AdapterError, ErrorCode, SnapshotSurface, WindowInfo};

pub(super) fn request_if_needed<T>(
    mut error: AdapterError,
    window: &WindowInfo,
    surface: SnapshotSurface,
    deadline: std::time::Instant,
) -> Result<T, AdapterError> {
    if !eligible(&error, surface) {
        return Err(error);
    }
    let Some(instance) = window.process_instance.as_deref() else {
        return Err(error);
    };
    let pid = crate::system::process::process_identity::to_pid_t(window.pid)?;
    if crate::tree::renderer_probe::activation_required(pid, instance, false, true, deadline)? {
        if let Some(details) = error
            .details
            .as_mut()
            .and_then(serde_json::Value::as_object_mut)
        {
            details.insert(
                "renderer_accessibility_activation_required".into(),
                true.into(),
            );
        }
    }
    Err(error)
}

fn eligible(error: &AdapterError, surface: SnapshotSurface) -> bool {
    surface == SnapshotSurface::Window
        && error.code == ErrorCode::ActionNotSupported
        && error
            .details
            .as_ref()
            .and_then(|details| details["kind"].as_str())
            == Some("window_without_accessibility_element")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_verified_missing_window_requests_a_probe() {
        let error = AdapterError::new(ErrorCode::ActionNotSupported, "missing window")
            .with_details(serde_json::json!({"kind": "window_without_accessibility_element"}));
        assert!(eligible(&error, SnapshotSurface::Window));
        assert!(!eligible(&error, SnapshotSurface::Menubar));
        assert!(!eligible(&error, SnapshotSurface::Focused));
        for code in [
            ErrorCode::PermDenied,
            ErrorCode::Timeout,
            ErrorCode::StaleRef,
        ] {
            let mut other = error.clone();
            other.code = code;
            assert!(!eligible(&other, SnapshotSurface::Window));
        }
        assert!(!eligible(
            &AdapterError::new(ErrorCode::ActionNotSupported, "other"),
            SnapshotSurface::Window
        ));
    }
}
