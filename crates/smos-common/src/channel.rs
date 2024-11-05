#[derive(Debug, Copy, Clone)]
pub struct Channel {
    pub ntfn: sel4::cap::Notification,
    // @alwin: Should this have a handle?
    // pub hndl: LocalHandle<ChannelHandle>,
}
