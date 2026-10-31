use crate::tree::{AXElement, renderer_probe, surface_read};
use agent_desktop_core::AdapterError;
use serde::Serialize;
use serde_json::{Value, json};
use std::time::Instant;

pub(super) fn read(application: &AXElement, deadline: Instant) -> Value {
    let windows = surface_read::elements(application, "AXWindows", deadline).map(|v| v.len());
    let children = surface_read::elements(application, "AXChildren", deadline).map(|children| {
        let roles: Vec<_> = children
            .iter()
            .take(16)
            .map(|child| outcome(surface_read::string(child, "AXRole", deadline)))
            .collect();
        json!({"count": children.len(), "roles": roles, "roles_truncated": children.len() > 16})
    });
    json!({
        "windows": outcome(windows),
        "children": outcome(children),
        "manual": attribute(application, renderer_probe::MANUAL, deadline),
        "enhanced": attribute(application, renderer_probe::ENHANCED, deadline),
    })
}

fn attribute(application: &AXElement, attribute: &str, deadline: Instant) -> Value {
    json!({
        "settable": outcome(renderer_probe::attribute_supported(application, attribute, deadline)),
        "value": outcome(surface_read::boolean(application, attribute, deadline)),
    })
}

fn outcome<T: Serialize>(result: Result<T, AdapterError>) -> Value {
    match result {
        Ok(value) => json!({"value": value}),
        Err(error) => json!({"error": error.message, "details": error.details}),
    }
}
