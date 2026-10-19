#version 330 core
layout (location = 0) in vec2 a_pos;
layout (location = 1) in vec2 a_local;

out vec2 v_local;

uniform mat4 u_proj;

void main() {
    v_local = a_local;
    gl_Position = u_proj * vec4(a_pos, 0.0, 1.0);
}
