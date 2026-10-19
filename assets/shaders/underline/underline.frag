#version 330 core
// x is the absolute window x, y is local to the strip in [0, u_band_height].
in vec2 v_local;
out vec4 frag_color;

const float TAU = 6.28318530717958647692;

// Matches glyph::UnderlineStyle's discriminants.
const int STYLE_STRAIGHT = 1;
const int STYLE_DOUBLE = 2;
const int STYLE_CURLY = 3;
const int STYLE_DOTTED = 4;
const int STYLE_DASHED = 5;

uniform int u_style;
uniform float u_thickness;
uniform vec4 u_color;
uniform float u_band_height;
// Dash plus gap for dotted/dashed, full wavelength for curly.
uniform float u_period;
uniform float u_dash;


const float AA = 0.5;

float band_coverage(float y, float center, float half_thickness) {
    return 1.0 - smoothstep(half_thickness - AA, half_thickness + AA, abs(y - center));
}

void main() {
    float center = u_band_height * 0.5;
    float coverage = 1.0;

    if (u_style == STYLE_DOUBLE) {
        float y = v_local.y;
        coverage = float(y < u_thickness || y > u_band_height - u_thickness);
    } else if (u_style == STYLE_DOTTED || u_style == STYLE_DASHED) {
        coverage = 1.0 - step(u_dash, mod(v_local.x, u_period));

    } else if (u_style == STYLE_CURLY) {
        // Leave room for the antialiasing falloff so the band does not clip
        // the crests.
        float amplitude = center - u_thickness * 0.5 - AA;
        float frequency = TAU / u_period;
        float wave_y = center + amplitude * sin(v_local.x * frequency);
        float slope = amplitude * frequency * cos(v_local.x * frequency);
        // Scaling by the slope turns the vertical distance into an
        // approximate perpendicular one, so steep parts keep the full width.
        float d = abs(v_local.y - wave_y) / sqrt(1.0 + slope * slope);
        coverage = band_coverage(d, 0.0, u_thickness * 0.5);
    }

    if (coverage <= 0.0) discard;
    frag_color = vec4(u_color.rgb, u_color.a * coverage);
}
