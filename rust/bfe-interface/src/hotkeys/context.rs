//! The actor or occupied seat selects one set of Battlefront II controls.
use std::collections::BTreeMap;

pub type BindingMap = BTreeMap<String, Vec<String>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ControlContext {
    Infantry,
    Vehicle,
    Starfighter,
    Jedi,
    Turret,
}

impl ControlContext {
    pub const ALL: [Self; 5] = [Self::Infantry, Self::Vehicle, Self::Starfighter, Self::Jedi, Self::Turret];

    pub(crate) fn bindable(code: &str) -> bool {
        super::INPUT_CODES.contains(&code) && !matches!(code, "Escape" | "Backspace" | "F11" | "F12")
    }

    pub fn defaults(self) -> BindingMap {
        super::swbf2::controls(self.tab())
            .iter()
            .filter_map(|control| {
                Some((control.row.action?.to_owned(), control.keys.iter().map(|key| (*key).to_owned()).collect()))
            })
            .collect()
    }

    pub fn bindings(self, settings: &crate::Settings) -> BindingMap {
        let mut result = self.defaults();
        if let Some(overrides) = settings.swbf2_bindings.get(&self) {
            for (action, keys) in &mut result {
                if let Some(codes) = overrides.get(action) {
                    *keys = codes.iter().filter(|code| Self::bindable(code)).cloned().collect();
                }
            }
        }
        result
    }

    /// Keep only effective overrides for supported actions on a marked native record.
    pub fn saved_overrides(self, mut overrides: BindingMap) -> BindingMap {
        let defaults = self.defaults();
        overrides.retain(|action, codes| {
            codes.retain(|code| Self::bindable(code));
            defaults.get(action).is_some_and(|default| default != codes)
        });
        overrides
    }

    pub fn rebind(self, settings: &mut crate::Settings, action: &str, code: Option<&str>) {
        let defaults = self.defaults();
        if !defaults.contains_key(action) || code.is_some_and(|key| !Self::bindable(key)) {
            return;
        }
        let mut bindings = self.bindings(settings);
        if let Some(code) = code {
            for keys in bindings.values_mut() {
                keys.retain(|key| key != code);
            }
        }
        bindings.insert(action.to_owned(), code.into_iter().map(str::to_owned).collect());
        bindings.retain(|action, keys| defaults.get(action) != Some(keys));
        settings.set_swbf2_bindings(self, bindings);
    }

    pub fn tab(self) -> &'static str {
        match self {
            Self::Infantry => "infantry",
            Self::Vehicle => "vehicle",
            Self::Starfighter => "starfighter",
            Self::Jedi => "jedi",
            Self::Turret => "turret",
        }
    }

    pub fn from_tab(tab: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|context| context.tab() == tab)
    }
}
