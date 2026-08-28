fn component_id(value: &str) -> Result<ComponentId> {
    ComponentId::parse(value).map_err(Into::into)
}

fn target_id(component: &str, action: &str) -> Result<TargetId> {
    Ok(TargetId::new(
        component_id(component)?,
        ActionId::parse(action)?,
    ))
}
