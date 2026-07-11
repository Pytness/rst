#version 330 core
layout (location = 0) in vec2 a_pos;
layout (location = 1) in vec2 a_uv;
layout (location = 2) in vec3 a_color;
layout (location = 3) in float a_is_color;

out vec2 v_uv;
out vec3 v_color;
flat out int v_is_color;

uniform mat4 u_proj;

void main() {
    v_uv = a_uv;
    v_color = a_color;
    v_is_color = int(a_is_color + 0.5);
    gl_Position = u_proj * vec4(a_pos, 0.0, 1.0);
}
