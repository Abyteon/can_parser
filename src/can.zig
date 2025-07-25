pub const CanFrame = packed struct {
    id: u32,
    data: u32,
    flags: u32,
    dlc: u8,

    pub fn init(id: u32, data: u32, dlc: u8, flags: u32) @This() {
        return .{
            .id = id,
            .data = data,
            .dlc = dlc,
            .flags = flags,
        };
    }
};
