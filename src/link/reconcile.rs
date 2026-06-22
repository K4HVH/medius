use crate::protocol::opcode::BTN_COUNT;
use crate::types::{Button, ButtonAction, CatchMask};

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
    catch: CatchMask,
}

impl Default for DesiredState {
    fn default() -> Self {
        DesiredState {
            overrides: [Override::None; BTN_COUNT as usize],
            catch: CatchMask::empty(),
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
        // Clears injection overrides only. Catch is deliberately NOT cleared by reset(): the firmware
        // keeps a subscription streaming across a RESET (it's passive observation, not injection), so
        // dropping the host's view here would orphan a live EventStream into a silent hang. A catch
        // subscription ends only when its stream is dropped (unsubscribe) or the control PC goes silent.
        self.overrides = [Override::None; BTN_COUNT as usize];
    }

    /// The catch subscription mask the box should be streaming (re-asserted on reconnect).
    pub(crate) fn set_catch(&mut self, mask: CatchMask) {
        self.catch = mask;
    }

    pub(crate) fn catch(&self) -> CatchMask {
        self.catch
    }

    /// Idle = nothing for the keepalive to hold alive. A catch subscription counts, so the silence
    /// timer keeps being fed and the box keeps streaming while a stream is open.
    pub(crate) fn is_idle(&self) -> bool {
        self.catch.is_empty() && self.overrides.iter().all(|o| *o == Override::None)
    }

    pub(crate) fn held(&self) -> impl Iterator<Item = (Button, ButtonAction)> + '_ {
        self.overrides.iter().enumerate().filter_map(|(id, ov)| {
            let action = ov.as_action()?;
            let button = Button::from_id(id as u8)?;
            Some((button, action))
        })
    }
}
