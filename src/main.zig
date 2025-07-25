const std = @import("std");
const can_parser = @import("can_parser");
const canframe = @import("can.zig").CanFrame;

pub fn main() !void {
    // Prints to stderr, ignoring potential errors.
    std.debug.print("All your {s} are belong to us.\n", .{"codebase"});
    try can_parser.bufferedPrint();

    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    const allocator = gpa.allocator();
    defer _ = gpa.deinit();

    const file = try std.fs.cwd().openFile("../data/sample_can.bin", .{});
    defer file.close();

    const frame_size = @sizeOf(canframe);
    const max_frames = 1024;

    const buf = try allocator.alloc(u8, frame_size * max_frames);
    defer allocator.free(buf);

    const read_bytes = try file.readAll(buf);
    const actual_frames = read_bytes / frame_size;
    std.debug.print("Read {d} bytes, which is {d} frames of size {d}", .{
        read_bytes,
        actual_frames,
        frame_size,
    });

    const frame = canframe.init(0x123, 45, 8, 0x1);
    std.debug.print("Created frame: id={d}, dlc={d}, flags={x}\n", .{
        frame.id,
        frame.dlc,
        frame.flags,
    });

    // for (0..actual_frames) |i| {
    //     // const ptr = buf[i * frame_size .. (i + 1) * frame_size];
    //     const ptr: *const canframe = @ptrCast(buf[i * frame_size .. (i + 1) * frame_size]);
    //     const frame = ptr.*;
    //     std.debug.print("Frame {d}: id={d}, dlc={d}, flags={x}\n", .{
    //         i,
    //         frame.id,
    //         frame.dlc,
    //         frame.flags,
    //     });
    // }
}

test "simple test" {
    var list = std.ArrayList(i32).init(std.testing.allocator);
    defer list.deinit(); // Try commenting this out and see if zig detects the memory leak!
    try list.append(42);
    try std.testing.expectEqual(@as(i32, 42), list.pop());
}

test "fuzz example" {
    const Context = struct {
        fn testOne(context: @This(), input: []const u8) anyerror!void {
            _ = context;
            // Try passing `--fuzz` to `zig build test` and see if it manages to fail this test case!
            try std.testing.expect(!std.mem.eql(u8, "canyoufindme", input));
        }
    };
    try std.testing.fuzz(Context{}, Context.testOne, .{});
}
