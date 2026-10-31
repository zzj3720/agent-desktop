use crate::tree::renderer_probe;
use agent_desktop_core::{AdapterError, Deadline, DeliverySemantics, ErrorCode, ProcessIdentity};
use std::time::Duration;

const ENHANCED_ACTIVATION_DELAY: Duration = Duration::from_secs(2);

pub(crate) fn activate(process: ProcessIdentity, deadline: Deadline) -> Result<(), AdapterError> {
    let identity =
        crate::system::process::process_identity::require_core(&process).map_err(not_delivered)?;
    let application = crate::tree::element_for_pid(identity.pid());
    match activate_element(&application, deadline) {
        Err(error) if rejected_before_delivery(&error) => {
            activate_nested(&application, identity.pid(), deadline, error)
        }
        result => result,
    }
}

fn rejected_before_delivery(error: &AdapterError) -> bool {
    error.code == ErrorCode::ActionNotSupported
        && error.disposition == DeliverySemantics::not_delivered()
}

fn activate_nested(
    application: &crate::tree::AXElement,
    pid: i32,
    deadline: Deadline,
    mut original: AdapterError,
) -> Result<(), AdapterError> {
    let children =
        crate::tree::surface_read::elements(application, "AXChildren", instant(deadline)?)?;
    let mut seen = vec![application.clone()];
    let mut diagnostics = Vec::new();
    for child in children.into_iter().take(16) {
        if !crate::tree::element_dedupe::push_unique(&mut seen, child.clone()) {
            continue;
        }
        if crate::tree::properties::ax_ipc::pid(&child, deadline) != Ok(pid)
            || crate::tree::surface_read::string(&child, "AXRole", instant(deadline)?)?.as_deref()
                != Some("AXApplication")
        {
            continue;
        }
        let result = activate_element(&child, deadline);
        match result {
            Ok(()) => return Ok(()),
            Err(error)
                if rejected_before_delivery(&error)
                    || error.code == ErrorCode::PlatformNotSupported =>
            {
                diagnostics
                    .push(serde_json::json!({"message": error.message, "details": error.details}));
            }
            Err(error) => return Err(error),
        }
    }
    original.details = Some(serde_json::json!({"root": original.details, "nested": diagnostics}));
    Err(original)
}

fn activate_element(
    application: &crate::tree::AXElement,
    deadline: Deadline,
) -> Result<(), AdapterError> {
    let attribute = renderer_probe::activation_attribute(
        application,
        instant(deadline).map_err(not_delivered)?,
    )
    .map_err(not_delivered)?
    .ok_or_else(|| {
        not_delivered(AdapterError::not_supported(
            "renderer accessibility activation",
        ))
    })?;
    let current = crate::tree::surface_read::boolean(
        application,
        attribute,
        instant(deadline).map_err(not_delivered)?,
    )
    .map_err(not_delivered)?;
    if current == Some(true) {
        return Ok(());
    }
    let before = super::renderer_diagnostics::read(application, instant(deadline)?);
    let result = activate_with_fallback(attribute, |candidate| {
        if !renderer_probe::attribute_supported(
            application,
            candidate,
            instant(deadline).map_err(not_delivered)?,
        )? {
            return Err(not_delivered(AdapterError::not_supported(candidate)));
        }
        enable_attribute(application, candidate, deadline)
    });
    result.map_err(|mut error| {
        let after =
            instant(deadline).map(|until| super::renderer_diagnostics::read(application, until));
        error.details = Some(serde_json::json!({
            "activation": error.details,
            "before": before,
            "after": after.ok(),
        }));
        error
    })
}

fn activate_with_fallback(
    attribute: &str,
    mut enable: impl FnMut(&str) -> Result<(), AdapterError>,
) -> Result<(), AdapterError> {
    match enable(attribute) {
        Err(error)
            if attribute == renderer_probe::MANUAL
                && error.code == ErrorCode::ActionNotSupported
                && error.disposition == DeliverySemantics::not_delivered() =>
        {
            enable(renderer_probe::ENHANCED).map_err(|mut enhanced| {
                let attempts = serde_json::json!({
                    "manual": {"message": error.message, "details": error.details},
                    "enhanced": {"message": enhanced.message, "details": enhanced.details},
                });
                enhanced.details = Some(attempts);
                enhanced
            })
        }
        result => result,
    }
}

fn enable_attribute(
    application: &crate::tree::AXElement,
    attribute_name: &str,
    deadline: Deadline,
) -> Result<(), AdapterError> {
    use core_foundation::{base::TCFType, boolean::CFBoolean, string::CFString};

    let settle_delay = if attribute_name == renderer_probe::ENHANCED {
        ENHANCED_ACTIVATION_DELAY
    } else {
        Duration::ZERO
    };
    if deadline.remaining() <= settle_delay {
        return Err(not_delivered(deadline.timeout_error()));
    }
    let attribute = CFString::new(attribute_name);
    let error = crate::tree::properties::ax_ipc::set_attribute_value(
        application,
        attribute.as_concrete_TypeRef(),
        CFBoolean::true_value().as_CFTypeRef(),
        deadline,
    )?;
    let readback = crate::tree::surface_read::boolean(
        application,
        attribute_name,
        instant(deadline).map_err(uncertain)?,
    )
    .map_err(uncertain)?;
    let delivered = activation_delivered(application, attribute_name, error, readback)?;
    if !delivered {
        return Err(AdapterError::new(
            ErrorCode::ActionNotSupported,
            "Renderer did not enable accessibility activation",
        )
        .with_details(serde_json::json!({
            "attribute": attribute_name,
            "ax_error": error,
            "readback": readback,
        }))
        .with_disposition(DeliverySemantics::not_delivered()));
    }
    std::thread::sleep(deadline.remaining().min(settle_delay));
    if deadline.is_expired() {
        return Err(deadline
            .timeout_error()
            .with_disposition(DeliverySemantics::delivered_unverified()));
    }
    Ok(())
}

fn activation_delivered(
    application: &crate::tree::AXElement,
    attribute: &str,
    error: i32,
    readback: Option<bool>,
) -> Result<bool, AdapterError> {
    if readback == Some(true) {
        return Ok(true);
    }
    let delivered = crate::actions::ax_mutation::classify_result(
        application,
        attribute,
        "AXUIElementSetAttributeValue",
        error,
    )?;
    if delivered && readback == Some(false) && attribute == renderer_probe::ENHANCED {
        return Err(AdapterError::new(
            ErrorCode::ActionFailed,
            "Renderer accessibility activation was not reflected in the attribute value",
        )
        .with_disposition(DeliverySemantics::delivered_unverified()));
    }
    Ok(delivered)
}

fn uncertain(error: AdapterError) -> AdapterError {
    error.with_disposition(DeliverySemantics::uncertain())
}

fn instant(deadline: Deadline) -> Result<std::time::Instant, AdapterError> {
    std::time::Instant::now()
        .checked_add(deadline.remaining_slice(std::time::Duration::from_millis(250))?)
        .ok_or_else(|| AdapterError::new(ErrorCode::InvalidArgs, "Deadline is out of range"))
}

fn not_delivered(error: AdapterError) -> AdapterError {
    error.with_disposition(DeliverySemantics::not_delivered())
}

#[cfg(test)]
#[path = "renderer_activation_tests.rs"]
mod tests;
