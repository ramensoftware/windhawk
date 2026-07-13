//! Domain-to-protocol conversions. Mechanical by design: the protocol crate is
//! self-contained and never re-exports domain types; this module is the one
//! place the two vocabularies meet.

use windhawk_core_domain as domain;
use windhawk_core_protocol as protocol;

pub fn metadata_to_protocol(m: domain::ModMetadata) -> protocol::ModMetadata {
    protocol::ModMetadata {
        version: m.version,
        id: m.id,
        github: m.github,
        twitter: m.twitter,
        homepage: m.homepage,
        compiler_options: m.compiler_options,
        license: m.license,
        donate_url: m.donate_url,
        name: m.name,
        description: m.description,
        author: m.author,
        include: m.include,
        exclude: m.exclude,
        architecture: m.architecture,
    }
}

pub fn settings_to_protocol(items: Vec<domain::SettingItem>) -> protocol::InitialSettings {
    items.into_iter().map(setting_item_to_protocol).collect()
}

fn setting_item_to_protocol(item: domain::SettingItem) -> protocol::InitialSettingItem {
    protocol::InitialSettingItem {
        key: item.key,
        value: setting_value_to_protocol(item.value),
        name: item.name,
        description: item.description,
        options: item.options.map(|options| {
            options
                .into_iter()
                .map(|(value, label)| std::collections::BTreeMap::from([(value, label)]))
                .collect()
        }),
    }
}

fn setting_value_to_protocol(value: domain::SettingValue) -> protocol::InitialSettingsValue {
    use domain::SettingValue as D;
    use protocol::InitialSettingsValue as P;
    match value {
        D::Bool(b) => P::Bool(b),
        D::Number(n) => P::Number(n),
        D::String(s) => P::String(s),
        D::NumberArray(v) => P::NumberArray(v),
        D::StringArray(v) => P::StringArray(v),
        D::Settings(items) => P::Settings(settings_to_protocol(items)),
        D::SettingsArray(arrays) => {
            P::SettingsArray(arrays.into_iter().map(settings_to_protocol).collect())
        }
    }
}
