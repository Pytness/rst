#version 330 core
in vec2 v_uv;
in vec3 v_color;
flat in int v_is_color;
out vec4 frag_color;

uniform sampler2D u_font;

void main() {
    vec4 tex = texture(u_font, v_uv);

    if (v_is_color == 1) {
        frag_color = tex;
    } else {
        float alpha = tex.r;
        frag_color = vec4(v_color, alpha);
    }
}
