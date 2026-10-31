use super::*;

#[test]
fn nested_activation_requires_explicit_rejection_before_delivery() {
    for code in [
        ErrorCode::ActionNotSupported,
        ErrorCode::Timeout,
        ErrorCode::PermDenied,
    ] {
        for disposition in [
            DeliverySemantics::not_delivered(),
            DeliverySemantics::uncertain(),
            DeliverySemantics::delivered_unverified(),
        ] {
            let error = AdapterError::new(code.clone(), "activation").with_disposition(disposition);
            assert_eq!(
                rejected_before_delivery(&error),
                code == ErrorCode::ActionNotSupported
                    && disposition == DeliverySemantics::not_delivered()
            );
        }
    }
}

#[test]
fn rejected_manual_activation_tries_supported_enhanced_activation() {
    let mut attempts = Vec::new();
    activate_with_fallback(renderer_probe::MANUAL, |attribute| {
        attempts.push(attribute.to_owned());
        if attribute == renderer_probe::MANUAL {
            Err(not_delivered(AdapterError::new(
                ErrorCode::ActionNotSupported,
                attribute,
            )))
        } else {
            Ok(())
        }
    })
    .unwrap();
    assert_eq!(attempts, [renderer_probe::MANUAL, renderer_probe::ENHANCED]);
}

#[test]
fn failed_fallback_keeps_both_diagnostics_and_final_disposition() {
    let error = activate_with_fallback(renderer_probe::MANUAL, |attribute| {
        Err(
            not_delivered(AdapterError::new(ErrorCode::ActionNotSupported, attribute))
                .with_details(serde_json::json!({"attribute": attribute})),
        )
    })
    .unwrap_err();
    let details = error.details.unwrap();
    assert_eq!(
        details["manual"]["details"]["attribute"],
        renderer_probe::MANUAL
    );
    assert_eq!(
        details["enhanced"]["details"]["attribute"],
        renderer_probe::ENHANCED
    );
    assert_eq!(error.disposition, DeliverySemantics::not_delivered());
}

#[test]
fn successful_or_uncertain_activation_never_tries_another_attribute() {
    for outcome in [
        Ok(()),
        Err(uncertain(AdapterError::new(
            ErrorCode::ActionNotSupported,
            "manual",
        ))),
        Err(not_delivered(AdapterError::permission_denied())),
    ] {
        let mut attempts = 0;
        let expected = outcome.as_ref().err().map(|error| error.code.clone());
        let result = activate_with_fallback(renderer_probe::MANUAL, |_| {
            attempts += 1;
            outcome.clone()
        });
        assert_eq!(attempts, 1);
        assert_eq!(result.err().map(|error| error.code), expected);
    }
}

#[test]
fn rejected_enhanced_activation_does_not_cycle_back_to_manual() {
    let mut attempts = 0;
    let error = activate_with_fallback(renderer_probe::ENHANCED, |attribute| {
        attempts += 1;
        Err(not_delivered(AdapterError::new(
            ErrorCode::ActionNotSupported,
            attribute,
        )))
    })
    .unwrap_err();
    assert_eq!(attempts, 1);
    assert_eq!(error.code, ErrorCode::ActionNotSupported);
}

#[test]
fn observed_activation_overrides_a_setters_not_implemented_response() {
    let application = crate::tree::AXElement(std::ptr::null_mut());
    assert!(
        activation_delivered(
            &application,
            renderer_probe::ENHANCED,
            accessibility_sys::kAXErrorNotImplemented,
            Some(true),
        )
        .unwrap()
    );
    assert!(
        !activation_delivered(
            &application,
            renderer_probe::ENHANCED,
            accessibility_sys::kAXErrorNotImplemented,
            Some(false),
        )
        .unwrap()
    );
}

#[test]
fn enhanced_activation_with_insufficient_budget_never_touches_native_handle() {
    let application = crate::tree::AXElement(std::ptr::null_mut());
    let deadline = Deadline::after(1_000).unwrap();
    let error = enable_attribute(&application, renderer_probe::ENHANCED, deadline).unwrap_err();
    assert_eq!(error.code, ErrorCode::Timeout);
    assert_eq!(error.disposition, DeliverySemantics::not_delivered());
}

#[test]
fn reported_success_with_disabled_readback_is_not_activation_success() {
    let application = crate::tree::AXElement(std::ptr::null_mut());
    let error = activation_delivered(
        &application,
        renderer_probe::ENHANCED,
        accessibility_sys::kAXErrorSuccess,
        Some(false),
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::ActionFailed);
    assert_eq!(error.disposition, DeliverySemantics::delivered_unverified());
}

#[test]
fn accepted_manual_write_with_disabled_readback_is_not_an_error() {
    let application = crate::tree::AXElement(std::ptr::null_mut());
    assert!(
        activation_delivered(
            &application,
            renderer_probe::MANUAL,
            accessibility_sys::kAXErrorSuccess,
            Some(false),
        )
        .unwrap()
    );
}

#[test]
fn missing_readback_keeps_legacy_success_and_preserves_real_failures() {
    let application = crate::tree::AXElement(std::ptr::null_mut());
    assert!(
        activation_delivered(
            &application,
            renderer_probe::MANUAL,
            accessibility_sys::kAXErrorSuccess,
            None,
        )
        .unwrap()
    );
    let error = activation_delivered(
        &application,
        renderer_probe::ENHANCED,
        accessibility_sys::kAXErrorCannotComplete,
        None,
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::AppUnresponsive);
    assert_eq!(error.disposition, DeliverySemantics::uncertain());
}
