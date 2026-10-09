use anyhow::Result;
use jig_contract::{ActionId, ComponentId, TargetId};

pub(super) fn component_id(value: &str) -> Result<ComponentId> {
    ComponentId::parse(value).map_err(Into::into)
}

pub(super) fn target_id(component: &str, action: &str) -> Result<TargetId> {
    Ok(TargetId::new(
        component_id(component)?,
        ActionId::parse(action)?,
    ))
}
