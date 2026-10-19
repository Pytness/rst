#version 330 core
layout (location = 0) in vec2 a_pos;
layout (location = 1) in vec2 a_local;
layout (location = 2) in vec4 a_color;
layout (location = 3) in uint a_code;

out vec2 v_local;
out vec4 v_color;
flat out uint v_code;

uniform mat4 u_proj;

void main() {
    v_local = a_local;
    v_color = a_color;
    v_code = a_code;
    gl_Position = u_proj * vec4(a_pos, 0.0, 1.0);
}
