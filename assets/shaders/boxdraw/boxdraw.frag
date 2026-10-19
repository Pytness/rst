#version 330 core
// Pixels from the cell's top-left corner, in [0, u_cell_size].
in vec2 v_local;
in vec4 v_color;
// A `boxdraw::boxdrawindex` code: category in the high bits, data in the low byte.
flat in uint v_code;
out vec4 frag_color;

uniform vec2 u_cell_size;

// Matches boxdraw::data.
const uint BDL = 1u << 8;
const uint BDA = 1u << 9;
const uint BBD = 1u << 10;
const uint BBL = 2u << 10;
const uint BBU = 3u << 10;
const uint BBR = 4u << 10;
const uint BBQ = 5u << 10;
const uint BRL = 6u << 10;
const uint BBS = 1u << 14;
const uint BDB = 1u << 15;

const uint LL = 1u << 0;
const uint LU = 1u << 1;
const uint LR = 1u << 2;
const uint LD = 1u << 3;
const uint DL = 1u << 4;
const uint DU = 1u << 5;
const uint DR = 1u << 6;
const uint DD = 1u << 7;

const uint TL = 1u << 0;
const uint TR = 1u << 1;
const uint BL = 1u << 2;
const uint BR = 1u << 3;

// Non-positive sizes cover nothing, like XftDrawRect.
bool rect(ivec2 p, int x, int y, int w, int h) {
    return p.x >= x && p.x < x + w && p.y >= y && p.y < y + h;
}

int div_round(int n, int d) {
    return (n + d / 2) / d;
}

bool has(uint bd, uint bits) {
    return (bd & bits) != 0u;
}

// Port of st's drawboxlines. Heavy is light + double drawn so they merge.
bool lines(ivec2 p, ivec2 size, uint bd) {
    int w = size.x;
    int h = size.y;

    // Stem about width/8 to match the underscore. Bold is 1.5x and at least
    // 1px thicker, but only once the cell is big enough to show it.
    int mwh = min(w, h);
    int base_s = max(1, div_round(mwh, 8));
    bool bold = has(bd, BDB) && mwh >= 6;
    int s = bold ? max(base_s + 1, div_round(3 * base_s, 2)) : base_s;

    // (w2, h2, s, s) is the center texel; lengths per direction include it.
    int w2 = div_round(w - s, 2);
    int h2 = div_round(h - s, 2);

    uint light = bd & (LL | LU | LR | LD);
    uint dbl = bd & (DL | DU | DR | DD);

    if (light != 0u) {
        // Skip the center texel at arcs and where light meets a double
        // corner, so it does not poke into the double's inner gap.
        bool multi_light = (light & (light - 1u)) != 0u;
        bool multi_double = (dbl & (dbl - 1u)) != 0u;
        int d = has(bd, BDA) || (multi_double && !multi_light) ? -s : 0;

        if (has(bd, LL) && rect(p, 0, h2, w2 + s + d, s)) return true;
        if (has(bd, LU) && rect(p, w2, 0, s, h2 + s + d)) return true;
        if (has(bd, LR) && rect(p, w2 - d, h2, w - w2 + d, s)) return true;
        if (has(bd, LD) && rect(p, w2, h2 - d, s, h - h2 + d)) return true;
    }

    // Clockwise, p extends the ray nearer the previous direction and n the
    // one nearer the next: shorter to avoid crossing other doubles, longer
    // to fill the outer corner.
    if (has(bd, DL)) {
        int p_ = has(bd, DD) ? -s : 0;
        int n_ = has(bd, DU) ? -s : (has(bd, DD) ? s : 0);
        if (rect(p, 0, h2 + s, w2 + s + p_, s)) return true;
        if (rect(p, 0, h2 - s, w2 + s + n_, s)) return true;
    }
    if (has(bd, DU)) {
        int p_ = has(bd, DL) ? -s : 0;
        int n_ = has(bd, DR) ? -s : (has(bd, DL) ? s : 0);
        if (rect(p, w2 - s, 0, s, h2 + s + p_)) return true;
        if (rect(p, w2 + s, 0, s, h2 + s + n_)) return true;
    }
    if (has(bd, DR)) {
        int p_ = has(bd, DU) ? -s : 0;
        int n_ = has(bd, DD) ? -s : (has(bd, DU) ? s : 0);
        if (rect(p, w2 - p_, h2 - s, w - w2 + p_, s)) return true;
        if (rect(p, w2 - n_, h2 + s, w - w2 + n_, s)) return true;
    }
    if (has(bd, DD)) {
        int p_ = has(bd, DR) ? -s : 0;
        int n_ = has(bd, DL) ? -s : (has(bd, DR) ? s : 0);
        if (rect(p, w2 + s, h2 - p_, s, h - h2 + p_)) return true;
        if (rect(p, w2 - s, h2 - n_, s, h - h2 + n_)) return true;
    }

    return false;
}

// Eighth blocks and quadrants, with st's integer splits.
bool blocks(ivec2 p, ivec2 size, uint category, uint data) {
    int w = size.x;
    int h = size.y;
    int x8 = int(data);

    if (category == BBD) {
        // Data is 8 - X for the lower X/8 block.
        int d = x8 * h / 8;
        return rect(p, 0, d, w, h - d);
    }
    if (category == BBU) return rect(p, 0, 0, w, x8 * h / 8);
    if (category == BBL) return rect(p, 0, 0, x8 * w / 8, h);
    if (category == BBR) {
        // Data is 8 - X for the right X/8 block.
        int d = x8 * w / 8;
        return rect(p, d, 0, w - d, h);
    }
    if (category == BBQ) {
        int w2 = w / 2;
        int h2 = h / 2;
        return (has(data, TL) && rect(p, 0, 0, w2, h2))
            || (has(data, TR) && rect(p, w2, 0, w - w2, h2))
            || (has(data, BL) && rect(p, 0, h2, w2, h - h2))
            || (has(data, BR) && rect(p, w2, h2, w - w2, h - h2));
    }

    return false;
}

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

    float coverage;
    if (has(v_code, BDL | BDA)) {
        coverage = float(lines(p, size, v_code));
    } else if (has(v_code, BBS)) {
        // st mixes fg into bg in quarters; blending fg at that alpha over
        // the already cleared background gives the same color.
        coverage = float(data) / 4.0;
    } else if (category == BRL) {
        coverage = float(braille(p, size, data));
    } else {
        coverage = float(blocks(p, size, category, data));
    }

    if (coverage <= 0.0) discard;
    frag_color = vec4(v_color.rgb, v_color.a * coverage);
}
