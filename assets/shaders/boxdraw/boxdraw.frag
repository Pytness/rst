#version 330 core
// Pixels from the cell's top-left corner, in [0, u_cell_size].
in vec2 v_local;
in vec4 v_color;
// A `boxdraw::boxdrawindex` code: category in the high bits, data in the low byte.
flat in uint v_code;
out vec4 frag_color;

uniform vec2 u_cell_size;

// Matches boxdraw::data's categories.
const uint BDB = 1u << 15;
const uint BRL = 6u << 10;

// Same integer splits as st, so dots land on whole pixels and adjacent
// cells tile without seams.
bool braille(ivec2 p, ivec2 size, uint dots) {
    int w1 = size.x / 2;
    int h1 = size.y / 4;
    int h2 = size.y / 2;
    int h3 = 3 * size.y / 4;

    int col = p.x < w1 ? 0 : 1;

    // Dots 1-6 run down each column, 7 and 8 were added below them later.
    int bit;
    if (p.y >= h3) {
        bit = 6 + col;
    } else {
        int row = p.y < h1 ? 0 : (p.y < h2 ? 1 : 2);
        bit = col * 3 + row;
    }

    return ((dots >> uint(bit)) & 1u) != 0u;
}

void main() {
    ivec2 p = ivec2(floor(v_local));
    ivec2 size = ivec2(u_cell_size);
    uint category = v_code & ~(BDB | 0xffu);
    uint data = v_code & 0xffu;

    bool covered = false;
    if (category == BRL) {
        covered = braille(p, size, data);
    }

    if (!covered) discard;
    frag_color = v_color;
}
