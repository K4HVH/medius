use crate::protocol::opcode::BTN_COUNT;
use crate::types::{Button, ButtonAction};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Override {
    #[default]
    None,
    Press,
    Force,
}

impl Override {
    pub(crate) fn as_action(self) -> Option<ButtonAction> {
        match self {
            Override::None => None,
            Override::Press => Some(ButtonAction::Press),
            Override::Force => Some(ButtonAction::ForceRelease),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DesiredState {
    overrides: [Override; BTN_COUNT as usize],
}

impl Default for DesiredState {
    fn default() -> Self {
        DesiredState {
            overrides: [Override::None; BTN_COUNT as usize],
        }
    }
}

impl DesiredState {
    pub(crate) fn apply(&mut self, button: Button, action: ButtonAction) {
        let slot = &mut self.overrides[button.as_id() as usize];
        *slot = match action {
            ButtonAction::Press => Override::Press,
            ButtonAction::ForceRelease => Override::Force,
            ButtonAction::SoftRelease => Override::None,
        };
    }

    pub(crate) fn clear(&mut self) {
        self.overrides = [Override::None; BTN_COUNT as usize];
    }

    pub(crate) fn is_idle(&self) -> bool {
        self.overrides.iter().all(|o| *o == Override::None)
    }

    pub(crate) fn held(&self) -> impl Iterator<Item = (Button, ButtonAction)> + '_ {
        self.overrides.iter().enumerate().filter_map(|(id, ov)| {
            let action = ov.as_action()?;
            let button = Button::from_id(id as u8)?;
            Some((button, action))
        })
    }
}
