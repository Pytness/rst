use glutin::config::{Config as GlutinConfig, ConfigTemplateBuilder, GetGlConfig, GlConfig};
use glutin::context::{
    ContextApi, ContextAttributesBuilder, NotCurrentContext, PossiblyCurrentContext, Version,
};
use glutin::display::GetGlDisplay;
use glutin::prelude::{GlDisplay, NotCurrentGlContext};
use glutin_winit::DisplayBuilder;
use winit::event_loop::ActiveEventLoop;
use winit::raw_window_handle::HasWindowHandle;
use winit::window::{Window, WindowAttributes};

pub enum GlDisplayCreationState {
    /// The display was not build yet.
    Builder(Box<DisplayBuilder>),
    /// The display was already created for the application.
    Init,
}

pub struct GlHandler {
    template: ConfigTemplateBuilder,
    pub gl_display: GlDisplayCreationState,
    pub gl_context: Option<PossiblyCurrentContext>,
    pub gl: Option<glow::Context>,
}

impl GlHandler {
    pub fn new(template: ConfigTemplateBuilder, display_builder: DisplayBuilder) -> Self {
        Self {
            template,
            gl_display: GlDisplayCreationState::Builder(Box::new(display_builder)),
            gl_context: None,
            gl: None,
        }
    }

    pub fn get_or_create_gl_window(
        &mut self,
        event_loop: &ActiveEventLoop,
    ) -> Option<(winit::window::Window, GlutinConfig)> {
        match &mut self.gl_display {
            GlDisplayCreationState::Builder(display_builder) => {
                let (window, gl_config) = match display_builder.clone().build(
                    event_loop,
                    self.template.clone(),
                    gl_config_picker,
                ) {
                    Ok((window, gl_config)) => (window.unwrap(), gl_config),
                    Err(e) => panic!("Failed to build display: {e}"),
                };

                self.gl_display = GlDisplayCreationState::Init;

                self.gl_context =
                    Some(create_gl_context(&window, &gl_config).treat_as_possibly_current());

                eprintln!("Picked a config with {} samples", gl_config.num_samples());

                Some((window, gl_config))
            }
            GlDisplayCreationState::Init => {
                let gl_config = self.gl_context.as_ref().unwrap().config();

                eprintln!("Reusing config with {} samples", gl_config.num_samples());

                match glutin_winit::finalize_window(event_loop, window_attributes(), &gl_config) {
                    Ok(window) => Some((window, gl_config)),
                    Err(_) => {
                        // self.exit_state = Err(err.into());
                        event_loop.exit();
                        None
                    }
                }
            }
        }
    }
}

pub fn window_attributes() -> WindowAttributes {
    Window::default_attributes()
        .with_transparent(true)
        .with_title("Glutin triangle gradient example (press Escape to exit)")
}

pub fn gl_config_picker(configs: Box<dyn Iterator<Item = GlutinConfig> + '_>) -> GlutinConfig {
    configs
        .reduce(|accum, config| {
            let transparency_check = config.supports_transparency().unwrap_or(false)
                & !accum.supports_transparency().unwrap_or(false);

            if transparency_check || config.num_samples() > accum.num_samples() {
                config
            } else {
                accum
            }
        })
        .unwrap()
}

fn create_gl_context(window: &Window, gl_config: &GlutinConfig) -> NotCurrentContext {
    let raw_window_handle = window.window_handle().ok().map(|wh| wh.as_raw());

    // The context creation part.
    let context_attributes = ContextAttributesBuilder::new().build(raw_window_handle);

    // Since glutin by default tries to create OpenGL core context, which may not be
    // present we should try gles.
    let fallback_context_attributes = ContextAttributesBuilder::new()
        .with_context_api(ContextApi::Gles(None))
        .build(raw_window_handle);

    // There are also some old devices that support neither modern OpenGL nor GLES.
    // To support these we can try and create a 2.1 context.
    let legacy_context_attributes = ContextAttributesBuilder::new()
        .with_context_api(ContextApi::OpenGl(Some(Version::new(2, 1))))
        .build(raw_window_handle);

    // Reuse the uncurrented context from a suspended() call if it exists, otherwise
    // this is the first time resumed() is called, where the context still
    // has to be created.
    let gl_display = gl_config.display();

    unsafe {
        gl_display
            .create_context(gl_config, &context_attributes)
            .unwrap_or_else(|_| {
                gl_display
                    .create_context(gl_config, &fallback_context_attributes)
                    .unwrap_or_else(|_| {
                        gl_display
                            .create_context(gl_config, &legacy_context_attributes)
                            .expect("failed to create context")
                    })
            })
    }
}
