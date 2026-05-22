pub const COLORS: [u32; 512] = {
    let mut arr = [0; 512];
    // 8 normal colors
    // 8 bright colors
    arr[0] = 0x000000; // 0
    arr[1] = 0xd86464; // 1
    arr[2] = 0x57d36d; // 2
    arr[3] = 0xd0d06a; // 3
    arr[4] = 0x6464ce; // 4
    arr[5] = 0xd763cc; // 5
    arr[6] = 0x56d2d2; // 6
    arr[7] = 0xd9d9d9; // 7

    // 8 bright colors
    arr[8] = 0x000000; // 0
    arr[9] = 0xd86464; // 1
    arr[10] = 0x57d36d; // 2
    arr[11] = 0xd0d06a; // 3
    arr[12] = 0x6464ce; // 4
    arr[13] = 0xd763cc; // 5
    arr[14] = 0x56d2d2; // 6
    arr[15] = 0xd9d9d9; // 7

    arr[256] = 0xcccccc;
    arr[257] = 0x555555;
    arr[258] = 0xe5e5e5;
    arr[259] = 0xF00000;

    arr
};
