use essential_core::err_on;
use essential_core::error::ServerError;
use fugaso_data::fugaso_action::ActionKind;
use maplit::{hashmap, hashset};
use std::collections::{HashMap, HashSet};

pub trait FSM {
    fn default(game_name: &str) -> Self;
    fn init(&mut self, action: ActionKind);
    fn client_act(&mut self, action: ActionKind) -> Result<ActionKind, ServerError>;
    fn reset(&mut self, action: ActionKind);
    fn server_act(&mut self, action: ActionKind) -> Result<ActionKind, ServerError>;
    fn current(&self) -> ActionKind;
}

pub struct SlotFSM {
    current: ActionKind,
    input: ActionKind,
    prev: ActionKind,
    game_name: String,
    transitions: HashMap<ActionKind, HashMap<ActionKind, ActionKind>>,
    client_acts: HashSet<ActionKind>,
}

impl SlotFSM {
    pub fn new(current: ActionKind, input: ActionKind, game_name: &str, prev: ActionKind) -> Self {
        let transitions = hashmap! {
            ActionKind::BET => hashmap! {
                ActionKind::BET => ActionKind::SPIN,
                ActionKind::BET_LINE => ActionKind::BET,
                ActionKind::BET_LINE_DENOM => ActionKind::BET,
                ActionKind::BET_LINE_REELS => ActionKind::BET,
                ActionKind::FREESPIN_START => ActionKind::FREE_SPIN,
                ActionKind::DROP_START => ActionKind::DROP,
            },
            ActionKind::COLLECT => hashmap! {
                ActionKind::COLLECT => ActionKind::BET,
                ActionKind::GAMBLE_PLAY => ActionKind::CLOSE,
                ActionKind::HALF_COLLECT => ActionKind::COLLECT,
            },
            ActionKind::SPIN => hashmap! {
                ActionKind::SPIN => ActionKind::CLOSE
            },
            ActionKind::FREE_SPIN => hashmap! {
                ActionKind::FREE_SPIN => ActionKind::CLOSE
            },
            ActionKind::DROP => hashmap! {
                ActionKind::DROP => ActionKind::CLOSE
            },
            ActionKind::RESPIN => hashmap! {
                ActionKind::RESPIN => ActionKind::CLOSE
            },
            ActionKind::BONUS => hashmap! {
                ActionKind::BONUS => ActionKind::BONUS
            },
            ActionKind::FREE_COLLECT => hashmap! {
                ActionKind::FREE_COLLECT => ActionKind::FREE_SPIN,
                ActionKind::GAMBLE_PLAY => ActionKind::CLOSE,
            },
            ActionKind::GAMBLE_END => hashmap! {
                ActionKind::COLLECT => ActionKind::BET,
            },
            ActionKind::GAMBLE_FREE_END => hashmap! {
                ActionKind::FREE_COLLECT => ActionKind::FREE_SPIN,
            },
            ActionKind::CLOSE => hashmap! {
                ActionKind::CLOSE => ActionKind::BET,
                ActionKind::COLLECT_START => ActionKind::COLLECT,
                ActionKind::FREE_COLLECT_START => ActionKind::FREE_COLLECT,
                ActionKind::RESPIN_START => ActionKind::RESPIN,
                ActionKind::GAMBLE_END => ActionKind::GAMBLE_END,
                ActionKind::GAMBLE_FREE_END => ActionKind::GAMBLE_FREE_END,
                ActionKind::FREESPIN_START => ActionKind::FREE_SPIN,
                ActionKind::DROP_START => ActionKind::DROP,
                ActionKind::BONUS_START => ActionKind::BONUS,
            }
        };

        let client_acts = hashset! {
                ActionKind::BET, ActionKind::SPIN, ActionKind::RESPIN,
                ActionKind::COLLECT, ActionKind::FREE_COLLECT,
                ActionKind::GAMBLE_PLAY, ActionKind::HALF_COLLECT,
                ActionKind::FREE_SPIN, ActionKind::BET_LINE,
                ActionKind::BET_LINE_DENOM, ActionKind::BET_LINE_REELS,
                ActionKind::DROP, ActionKind::BONUS
        };

        Self {
            prev,
            current,
            input,
            game_name: game_name.to_string(),
            transitions,
            client_acts,
        }
    }
}

impl FSM for SlotFSM {
    fn default(game_name: &str) -> Self {
        SlotFSM::new(ActionKind::BET, ActionKind::BET, game_name, ActionKind::BET)
    }

    fn init(&mut self, action: ActionKind) {
        self.current = action;
    }

    fn client_act(&mut self, action: ActionKind) -> Result<ActionKind, ServerError> {
        if self.client_acts.contains(&action) {
            self.input = action.clone();
            let tran = self.transitions.get(&self.current).ok_or_else(|| {
                err_on!(format!(
                    "Server transition for {:?} is not present - game:{:?}!",
                    action, self.game_name
                ))
            })?;
            self.prev = self.current.clone();
            self.current = tran
                .get(&action)
                .ok_or_else(|| {
                    err_on!(format!(
                        "Illegal state from:{:?} input:{:?} to:{:?} transition - game:{:?}!",
                        self.prev, action, self.current, self.game_name
                    ))
                })?
                .clone();
            Ok(self.current.clone())
        } else {
            Err(err_on!("Illegal client action"))
        }
    }

    fn reset(&mut self, action: ActionKind) {
        self.current = action.clone();
        self.input = action;
    }

    fn server_act(&mut self, action: ActionKind) -> Result<ActionKind, ServerError> {
        let tran = self.transitions.get(&self.current).ok_or(ServerError {
            file: file!(),
            line: line!(),
            message: format!(
                "Server transition for {:?} is not present game:{:?}! ",
                action, self.game_name
            ),
        })?;
        self.prev = self.current.clone();
        self.current = tran
            .get(&action)
            .ok_or_else(|| {
                err_on!(format!(
                    "Illegal state from:{:?} input:{:?} to:{:?} transition - game:{:?}!",
                    self.prev, action, self.current, self.game_name
                ))
            })?
            .clone();
        Ok(self.current.clone())
    }

    fn current(&self) -> ActionKind {
        self.current.clone()
    }
}
