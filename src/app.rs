use std::ffi::{CString, c_void};
use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::Instant;

use bitflags::Flags;
use glow::HasContext;
use glutin::config::ConfigTemplateBuilder;
use glutin::display::GetGlDisplay;
use glutin::prelude::{GlDisplay, PossiblyCurrentGlContext};
use glutin::surface::GlSurface;
use glutin_winit::{DisplayBuilder, GlWindow};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
use winit::window::WindowId;

use crate::colors::COLORS;
use crate::config::maxlatency;
use crate::font_registry::{FontRegistry, FontStyle};
use crate::gl_handler::GlHandler;
use crate::glyph::{Glyph, GlyphAttribute};
use crate::macros::macs::include_font;
use crate::renderers::{self, TextRenderer};
use crate::terminal::{IS_TRUECOL, Term, twrite_aborted};
use crate::text_manager::TermGlyph;
use crate::win::{TermWindow, WinMode};

pub struct AppState {
    gl_surface: glutin::surface::Surface<glutin::surface::WindowSurface>,
    window: winit::window::Window,
}

pub struct App<'a> {
    gl_handler: GlHandler,
    app_state: Option<AppState>,
    gl: Option<Rc<glow::Context>>,

    font_registry: FontRegistry,
    text_renderer: Option<TextRenderer<'a>>,
    quad_renderer: Option<renderers::QuadRenderer>,
    conf_font_size_px: u32,

    term: Term,
    win: TermWindow,
    ttyfd: i32,
    rfd: libc::fd_set,
}

impl<'a> App<'a> {
    pub fn new(
        term: Term,
        win: TermWindow,
        template: ConfigTemplateBuilder,
        display_builder: DisplayBuilder,
    ) -> Self {
        let mut term = term;

        let ttyfd = term.ttynew(None, Some("/bin/zsh"), None, None);
        println!("ttyfd: {ttyfd}");

        let mut font_registry = FontRegistry::new();

        font_registry.register_font(
            "CaskaydiaCove Nerd Font:size=10:antialias=true:autohint=true",
            include_font!("CaskaydiaCoveNerdFont-Regular.ttf"),
        );

        Self {
            gl_handler: GlHandler::new(template, display_builder),
            app_state: None,
            gl: None,

            font_registry,
            text_renderer: None,
            quad_renderer: None,
            conf_font_size_px: 16,

            term,
            win,
            ttyfd,
            rfd: unsafe { std::mem::zeroed() },
        }
    }

    /// SAFETY: This function should only be called after the OpenGL context has been created and made current in the `resumed` method.
    /// Calling this function before that will result in undefined behavior.
    pub unsafe fn gl(&self) -> &glow::Context {
        self.gl.as_ref().unwrap()
    }

    pub fn kpress(&mut self, event: KeyEvent) {
        if self.win.mode.contains(WinMode::MODE_KBDLOCK) {
            return;
        }

        // println!("key event: {:?}", event);

        let PhysicalKey::Code(code) = event.physical_key else {
            return;
        };

        // hightlight URLs when control held
        if code == KeyCode::ControlLeft {
            match event.state {
                ElementState::Pressed => {
                    println!("Control held");
                    // highlighturls();
                }
                ElementState::Released => {
                    println!("Control released");
                    // unhighlighturls();
                }
            }
        }

        // Released not relevant to shortcuts
        if event.state == ElementState::Released {
            return;
        }

        let _is_alt_screen = self.term.tisaltscr();

        // shortcuts
        for _shorcut in super::config::shortcuts {
            /*
             * TODO:
             * match shortcuts
             * if matches shortcut {
             * return;
             * }
             */
        }

        // custom keys from config
        /*
         * TODO:
         * if ((customkey = kmap(ksym, e->state))) {
         * 	ttywrite(customkey, strlen(customkey), 1);
         * 	return;
         * }
         */

        // composed string from input method
        let Some(text) = event.text_with_all_modifiers() else {
            return;
        };

        let bytes = text.as_bytes();
        let mut len = bytes.len();

        if len == 0 {
            return;
        }
        println!("Composed text ({}): {:?}", len, text);

        let mut buffer = [b'\0'; 64];
        buffer[..len].copy_from_slice(&bytes.iter().take(len).cloned().collect::<Vec<u8>>());

        const MOD1: bool = false;
        // TODO: if (len == 1 && e->state & Mod1Mask)
        if len == 1 && MOD1 {
            println!("Single character input: {}", buffer[0] as char);
            if self.win.mode.contains(WinMode::MODE_8BIT) || true {
                println!("8-bit mode enabled, treating input as 8-bit character");
                if buffer[0] < 0o177 {
                    let c = buffer[0] | 0x80;
                    len = (c as char).len_utf8();
                }
            } else {
                println!("8-bit mode disabled, treating input as UTF-8 character");
                buffer[1] = b'\0';
                buffer[0] = b'\x1b';
                len = 2;
            }
        }

        self.term.ttywrite(&buffer, len, true);
    }
    pub fn cmessage(&mut self) {}

    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        if size.width == self.win.w && size.height == self.win.h {
            return;
        }

        self.gl_resize(size);
        self.cresize(size.width, size.height);

        self.term
            .ttyresize(size.width as usize, size.height as usize);

        unsafe {
            self.quad_renderer = Some(renderers::QuadRenderer::new(
                self.gl.as_ref().unwrap().clone(),
                size.width as i32,
                size.height as i32,
            ));

            self.quad_renderer.as_ref().unwrap().clear_section(
                0,
                0,
                size.width as i32,
                size.height as i32,
                (0.0, 0.0, 0.0, 0.4),
            );

            self.text_renderer
                .as_mut()
                .unwrap()
                .set_viewport(size.width as i32, size.height as i32);

            self.gl()
                .viewport(0, 0, size.width as i32, size.height as i32);
        }
    }

    fn gl_resize(&mut self, size: PhysicalSize<u32>) {
        if let Some(AppState {
            gl_surface,
            window: _window,
        }) = self.app_state.as_ref()
        {
            let gl_context = self.gl_handler.gl_context.as_ref().unwrap();
            gl_surface.resize(
                gl_context,
                NonZeroU32::new(size.width).unwrap(),
                NonZeroU32::new(size.height).unwrap(),
            );
        } else {
            println!("Resize event received before GL surface was created, ignoring.");
        }
    }

    fn cresize(&mut self, width: u32, height: u32) {
        if width != 0 {
            self.win.w = width;
        }

        if height != 0 {
            self.win.h = height;
        }

        // TODO:
        let borderpx = 0;
        let mut col = (self.win.w - 2 * borderpx) / self.win.cw;
        let mut row = (self.win.h - 2 * borderpx) / self.win.ch;

        col = col.max(2);
        row = row.max(1);

        self.win.hborderpx =
            ((self.win.w - col * self.win.cw) as f64 * super::config::HALIGN) as u32;

        self.win.vborderpx =
            ((self.win.h - row * self.win.ch) as f64 * super::config::VALIGN) as u32;

        self.term.tresize(col as usize, row as usize);

        // xresize(col, row);

        self.term
            .ttyresize(self.win.tw as usize, self.win.th as usize);
    }

    pub fn visibility(&mut self) {}
    pub fn unmap(&mut self) {}
    pub fn expose(&mut self) {}
    pub fn focus(&mut self) {}
    pub fn bmotion(&mut self) {}
    pub fn bpress(&mut self) {}
    pub fn brelease(&mut self) {}

    pub fn draw(&mut self) {
        let mut cx = self.term.c.x;
        let mut ocx = self.term.ocx;
        let mut ocy = self.term.c.y;

        if !self.xstartdraw() {
            return;
        }

        ocx = ocx.max(0).min(self.term.col - 1);
        ocy = ocy.max(0).min(self.term.row - 1);

        if self.term.line[self.term.ocy][self.term.ocx]
            .mode
            .contains(GlyphAttribute::ATTR_WDUMMY)
        {
            self.term.ocx -= 1;
        }

        if self.term.line[self.term.c.y][cx]
            .mode
            .contains(GlyphAttribute::ATTR_WDUMMY)
        {
            cx -= 1;
        }

        self.drawregion(0, 0, self.term.col, self.term.row);

        let line = self.term.line[self.term.ocy].clone();
        let g = &self.term.line[self.term.c.y][cx].clone();
        let og = &raw mut self.term.line[self.term.ocy][self.term.ocx];

        self.xdrawcursor(
            cx as usize,
            self.term.c.y as usize,
            &g,
            self.term.ocx as usize,
            self.term.ocy as usize,
            og,
            &line,
            self.term.col as usize,
        );

        self.term.ocx = cx;
        self.term.ocy = self.term.c.y;

        self.xfinishdraw();

        if ocx != self.term.ocx || ocy != self.term.ocy {
            self.xximspot(self.term.ocx as usize, self.term.ocy as usize);
        }
    }

    fn xstartdraw(&self) -> bool {
        return self.win.mode.contains(WinMode::MODE_VISIBLE);
    }

    fn drawregion(&mut self, x1: i32, y1: i32, x2: usize, y2: usize) {
        println!("Drawing region: ({}, {}) to ({}, {})", x1, y1, x2, y2);
        self.xstartimagedraw(&self.term.dirty, self.term.row);

        for y in y1 as usize..y2 {
            if !self.term.dirty[y] {
                continue;
            }

            self.term.dirty[y] = false;
            let line = &self.term.line[y].clone();
            println!("Drawing line {}", y);
            self.xdrawline(line, x1, y, x2);
        }

        self.xfinishimagedraw();
    }

    fn xdrawcursor(
        &mut self,
        _cx: usize,
        _cy: usize,
        _g: &Glyph,
        ox: usize,
        oy: usize,
        og: *mut Glyph,
        line: &[Glyph],
        len: usize,
    ) {
        // remove the old cursor
        if self.term.selected(ox, oy) {
            unsafe { (*og).mode.toggle(GlyphAttribute::ATTR_REVERSE) };
        }

        // Redraw the line where cursor was previously.
        // It will restore the ligatures broken by the cursor.

        self.xdrawline(line, 0, oy, len);
    }

    fn xfinishdraw(&self) {
        // TODO:
        println!("Finished drawing");
    }

    fn xximspot(&self, _ocx: usize, _ocy: usize) {
        // TODO:
        println!("xximspot");
    }

    fn xdrawline(&mut self, line: &[Glyph], x1: i32, y1: usize, x2: usize) {
        let _i = 0;
        let _x = 0;
        let _ox = 0;
        let _numspecs = 0;

        let _base: Glyph = Glyph::default();
        let _new: Glyph = Glyph::default();

        fn u32_to_tuple(color: u32) -> (u8, u8, u8) {
            let color = if !IS_TRUECOL(color) {
                COLORS[color as usize]
            } else {
                color
            };

            let r = ((color >> 16) & 0xFF) as u8;
            let g = ((color >> 8) & 0xFF) as u8;
            let b = (color & 0xFF) as u8;

            (r, g, b)
        }

        let glyphs = line[x1 as usize..x2].to_vec();
        let glyphs: Vec<TermGlyph> = glyphs
            .into_iter()
            .map(|g| TermGlyph {
                char: g.u,
                fg_color: u32_to_tuple(g.fg),
                bg_color: u32_to_tuple(g.bg),
                font_style: FontStyle::Regular,
            })
            .collect();

        unsafe {
            self.quad_renderer.as_ref().unwrap().with(|| {
                println!("Pixw: {}, Pixh: {}", self.term.pixw, self.term.pixh);
                let proj = ortho(self.term.pixw as f32, self.term.pixh as f32);

                self.text_renderer
                    .as_mut()
                    .unwrap()
                    .draw_glyphs(&glyphs, y1 as i32, x1 as i32, &proj);
            });
        }
    }

    fn xstartimagedraw(&self, _dirty: &[bool], _row: usize) {
        // todo!()
    }

    fn xfinishimagedraw(&self) {
        // todo!()
    }
}

impl<'a> ApplicationHandler for App<'a> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        static mut PTR: *mut App = std::ptr::null_mut::<App>();
        unsafe {
            let void = self as *mut App as *mut c_void;

            PTR = void as *mut App;
        }

        let draw: Box<dyn FnMut() -> ()> = Box::new(move || unsafe {
            (*PTR).draw();
        });

        self.term.draw = Some(draw);

        // make draw static

        let gl_window = self.gl_handler.get_or_create_gl_window(event_loop);

        if gl_window.is_none() {
            return;
        }

        let (window, gl_config) = gl_window.unwrap();

        let attrs = window
            .build_surface_attributes(Default::default())
            .expect("Failed to build surface attributes");

        let gl_surface = unsafe {
            gl_config
                .display()
                .create_window_surface(&gl_config, &attrs)
                .expect("Failed to create surface")
        };

        let gl_context = self.gl_handler.gl_context.as_ref().unwrap();
        gl_context.make_current(&gl_surface).unwrap();

        let gl = unsafe {
            glow::Context::from_loader_function(|s| {
                let symbol = CString::new(s).unwrap();
                gl_config.display().get_proc_address(symbol.as_c_str())
            })
        };

        self.gl = Some(Rc::new(gl));

        self.win.mode.insert(WinMode::MODE_VISIBLE);

        // FontRegistry must outlive TextRenderer
        self.text_renderer.get_or_insert_with(|| unsafe {
            // This is safe because the font registry is owned by the App struct
            // and will not be dropped while the TextRenderer is still in use.
            let font_registry: &'a FontRegistry = &*(&self.font_registry as *const _);

            let width = window.inner_size().width as i32;
            let height = window.inner_size().height as i32;
            TextRenderer::<'a>::new(
                self.gl.as_ref().unwrap().clone(),
                font_registry,
                self.conf_font_size_px,
                (width, height),
            )
        });

        let font_size = self.text_renderer.as_ref().unwrap().font_size();
        self.win.cw = font_size.width as u32;
        self.win.ch = font_size.height as u32;

        self.quad_renderer.get_or_insert_with(|| unsafe {
            renderers::QuadRenderer::new(
                self.gl.as_ref().unwrap().clone(),
                window.inner_size().width as i32,
                window.inner_size().height as i32,
            )
        });

        self.app_state = Some(AppState { gl_surface, window });
    }

    fn new_events(&mut self, event_loop: &ActiveEventLoop, _cause: winit::event::StartCause) {
        let timeout: f64 = maxlatency as f64 / 1000.0;
        unsafe {
            // TODO: implement missing timeout handling
            let tv: libc::timespec = libc::timespec {
                tv_sec: timeout as libc::time_t,
                tv_nsec: ((timeout - timeout.floor()) * 1e9) as libc::c_long,
            };

            libc::FD_ZERO(&mut self.rfd);
            libc::FD_SET(self.ttyfd, &mut self.rfd);

            if libc::pselect(
                self.ttyfd + 1,
                &mut self.rfd,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &tv,
                std::ptr::null(),
            ) < 0
            {
                if *libc::__errno_location() != libc::EINTR {
                    panic!("pselect failed: {}", std::io::Error::last_os_error());
                }
            }

            let ttyin = libc::FD_ISSET(self.ttyfd, &mut self.rfd);

            if ttyin || unsafe { twrite_aborted } {
                self.term.ttyread();
            }

            // set winit event loop to rerun in 10ms
            let timeout = std::time::Duration::from_millis(maxlatency as u64);
            let control = ControlFlow::WaitUntil(std::time::Instant::now() + timeout);
            event_loop.set_control_flow(control);
        }

        self.draw();
        if let Some(window) = self.app_state.as_ref().map(|s| &s.window) {
            window.request_redraw();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _id: WindowId,
        event: winit::event::WindowEvent,
    ) {
        println!("~~~~~~~~~~~~~~!@#!@#!@#Received window event");

        match event {
            WindowEvent::KeyboardInput {
                event,
                is_synthetic: false,
                ..
            } => self.kpress(event),
            WindowEvent::Resized(size) => self.resize(size),

            WindowEvent::RedrawRequested => {
                let start = Instant::now();
                if let Some(AppState {
                    gl_surface,
                    window: _,
                }) = &self.app_state
                {
                    let gl_context = self.gl_handler.gl_context.as_ref().unwrap();

                    unsafe {
                        self.quad_renderer.as_ref().unwrap().render();
                    }
                    gl_surface.swap_buffers(gl_context).unwrap();
                }

                let duration = start.elapsed();
                println!("Redrawn in {} ms", duration.as_millis());
            }

            WindowEvent::CloseRequested => {
                event_loop.exit();
            }

            _ => (),
        }
    }
}

fn ortho(width: f32, height: f32) -> [f32; 16] {
    #[rustfmt::skip]
    return [
        2.0 / width, 0.0, 0.0, 0.0,
        0.0, -2.0 / height, 0.0, 0.0,
        0.0, 0.0, -1.0, 0.0,
        -1.0, 1.0, 0.0, 1.0,
    ];
}
