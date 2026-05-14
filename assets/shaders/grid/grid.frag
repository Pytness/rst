#version 330 core

out vec4 frag_color;

uniform vec2 u_resolution;   // window size in pixels
uniform vec2 u_grid_size;    // (W, H) size of each cell in pixels
uniform vec2 u_offset;       // grid offset in pixels
uniform vec4 u_bg_color;     // background color
uniform vec4 u_line_color;   // grid line color
uniform float u_line_width;  // line thickness in pixels

void main() {
    // Pixel position in window space
    vec2 p = gl_FragCoord.xy - u_offset;

    // Position inside the current cell, wrapped to [0, cell_size)
    vec2 cell = mod(p, u_grid_size);
    if (cell.x < 0.0) cell.x += u_grid_size.x;
    if (cell.y < 0.0) cell.y += u_grid_size.y;

    float global_x = gl_FragCoord.x;
    float global_y = gl_FragCoord.y;

    if (global_x < u_offset.x || global_y < u_offset.y || global_x > u_resolution.x - u_offset.x || global_y > u_resolution.y - u_offset.y) {
        frag_color = u_bg_color;
        return;
    }

    // Distance to nearest vertical / horizontal grid line
    float dx = min(cell.x, u_grid_size.x - cell.x);
    float dy = min(cell.y, u_grid_size.y - cell.y);
    float d = min(dx, dy);

    // Sharp grid lines
    float line = 1.0 - step(u_line_width, d);

    frag_color = mix(u_bg_color, u_line_color, line);
}
