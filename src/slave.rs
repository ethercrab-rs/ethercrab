use crate::al_status::AlStatus;

#[derive(Clone, Debug)]
pub struct Slave {
    pub configured_address: u16,
    pub state: AlStatus,
}

impl Slave {
    pub fn new(configured_address: u16, state: AlStatus) -> Self {
        Self {
            configured_address,
            state,
        }
    }
}
