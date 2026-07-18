use std::ffi::{CString, c_void};
use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::{Duration, Instant};

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
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
use winit::window::WindowId;

use crate::colors::COLORS;
use crate::config::{self, MAXLATENCY, MINLATENCY};
use crate::font_registry::{FontRegistry, FontStyle};
use crate::gl_handler::GlHandler;
use crate::glyph::{Glyph, GlyphAttribute};
use crate::keymap::kmap;
use crate::macros::macs::include_font;
use crate::renderers::{self, TextRenderer};
use crate::term_state::SU;
use crate::terminal::{IS_TRUECOL, TWRITE_ABORTED, Term};
use crate::text_manager::TermGlyph;
use crate::time_this;
use crate::win::WinMode;

pub struct AppState {
    gl_surface: glutin::surface::Surface<glutin::surface::WindowSurface>,
    window: winit::window::Window,
}

pub struct App<'a> {
    gl_handler: GlHandler,
    app_state: Option<AppState>,
    gl: Option<Rc<glow::Context>>,

    keyboard_modifiers: ModifiersState,

    font_registry: FontRegistry,
    text_renderer: Option<TextRenderer<'a>>,
    quad_renderer: Option<renderers::QuadRenderer>,
    conf_font_size_px: u32,
    loop_timeout: f64,

    // Mirrors st's `drawing`/`trigger`: once tty or window input starts a
    // burst, we hold off redrawing until things go idle (or maxlatency runs
    // out) to avoid tearing/flicker on rapid output.
    drawing: bool,
    trigger: Instant,
    // Mirrors st's `xev`: set from `window_event` for any real window event
    // (but not our own RedrawRequested), consumed at the top of `new_events`.
    xev_pending: bool,

    term: Term,
    ttyfd: i32,
    rfd: libc::fd_set,
    last_blink: Instant,
}

impl<'a> App<'a> {
    pub fn new(
        term: Term,
        template: ConfigTemplateBuilder,
        display_builder: DisplayBuilder,
    ) -> Self {
        let mut term = term;

        let ttyfd = term.ttynew(None, Some(config::SHELL), None, None);
        println!("ttyfd: {ttyfd}");

        let mut font_registry = FontRegistry::new();

        font_registry.register_font(
            "CaskaydiaCove Nerd Font:size=10:antialias=true:autohint=true",
            include_font!("CaskaydiaCoveNerdFont-Regular.ttf"),
        );

        font_registry.register_font(
            "Noto Color Emoji:size=10:antialias=true:autohint=true",
            include_font!("SymbolsNerdFont-Regular.ttf"),
        );

        term.colors.load_colors();

        Self {
            gl_handler: GlHandler::new(template, display_builder),
            app_state: None,
            gl: None,

            keyboard_modifiers: Default::default(),

            font_registry,
            text_renderer: None,
            quad_renderer: None,
            conf_font_size_px: 16,
            loop_timeout: 0.0,

            drawing: false,
            trigger: Instant::now(),
            xev_pending: false,

            term,
            ttyfd,
            rfd: unsafe { std::mem::zeroed() },
            last_blink: Instant::now(),
        }
    }

    /// SAFETY: This function should only be called after the OpenGL context has been created and made current in the `resumed` method.
    /// Calling this function before that will result in undefined behavior.
    pub unsafe fn gl(&self) -> &glow::Context {
        self.gl.as_ref().unwrap()
    }

    pub fn kpress(&mut self, event: KeyEvent) {
        if self.term.win.mode.contains(WinMode::KbdLock) {
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

        let _is_alt_screen = self.term.state.tisaltscr();

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
        if let Some(customkey) = kmap(code, self.keyboard_modifiers) {
            let customkey_str = customkey.to_str().unwrap();
            self.term
                .ttywrite(customkey_str.as_bytes(), customkey_str.len(), true);
            return;
        }

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
            if self.term.win.mode.contains(WinMode::EightBit) || true {
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
        if size.width == self.term.win.w && size.height == self.term.win.h {
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
                .set_viewport(&self.term.win);

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
        let term = &mut self.term;

        if width != 0 {
            term.win.w = width;
        }

        if height != 0 {
            term.win.h = height;
        }

        let mut col = (term.win.w - 2 * config::BORDERPX) / term.win.cw;
        let mut row = (term.win.h - 2 * config::BORDERPX) / term.win.ch;

        col = col.max(2);
        row = row.max(1);

        term.win.hborderpx =
            ((term.win.w - col * term.win.cw) as f64 * super::config::HALIGN) as u32;

        term.win.vborderpx =
            ((term.win.h - row * term.win.ch) as f64 * super::config::VALIGN) as u32;

        term.state.tresize(col as usize, row as usize);

        // TODO: xresize(col, row);
        // included in xresize (not implemented yet)
        term.win.tw = col * term.win.cw;
        term.win.th = row * term.win.ch;

        term.ttyresize(term.win.tw as usize, term.win.th as usize);
    }

    pub fn visibility(&mut self) {}
    pub fn unmap(&mut self) {}
    pub fn expose(&mut self) {}
    pub fn focus(&mut self) {}
    pub fn bmotion(&mut self) {}
    pub fn bpress(&mut self) {}
    pub fn brelease(&mut self) {}

    pub fn draw(&mut self) {
        let mut cx = self.term.state.c.x;
        let mut ocx = self.term.state.ocx;
        let mut ocy = self.term.state.c.y;

        if !self.xstartdraw() {
            return;
        }

        ocx = ocx.max(0).min(self.term.state.col - 1);
        ocy = ocy.max(0).min(self.term.state.row - 1);

        self.term.state.ocx = ocx;
        self.term.state.ocy = ocy;

        if self.term.state.line[self.term.state.ocy][self.term.state.ocx]
            .mode
            .contains(GlyphAttribute::ATTR_WDUMMY)
        {
            self.term.state.ocx -= 1;
        }

        if self.term.state.line[self.term.state.c.y][cx]
            .mode
            .contains(GlyphAttribute::ATTR_WDUMMY)
        {
            cx -= 1;
        }

        self.drawregion(0, 0, self.term.state.col, self.term.state.row);

        let line = self.term.state.line[self.term.state.ocy].clone();
        let g = &self.term.state.line[self.term.state.c.y][cx].clone();
        let og = &raw mut self.term.state.line[self.term.state.ocy][self.term.state.ocx];

        self.xdrawcursor(
            cx as usize,
            self.term.state.c.y as usize,
            &g,
            self.term.state.ocx as usize,
            self.term.state.ocy as usize,
            og,
            &line,
            self.term.state.col as usize,
        );

        self.term.state.ocx = cx;
        self.term.state.ocy = self.term.state.c.y;

        self.xfinishdraw();

        if ocx != self.term.state.ocx || ocy != self.term.state.ocy {
            self.xximspot(self.term.state.ocx as usize, self.term.state.ocy as usize);
        }
    }

    fn xstartdraw(&self) -> bool {
        return self.term.win.mode.contains(WinMode::Visible);
    }

    fn drawregion(&mut self, x1: i32, y1: i32, x2: usize, y2: usize) {
        self.xstartimagedraw(&self.term.state.dirty, self.term.state.row);

        for y in y1 as usize..y2 {
            if !self.term.state.dirty[y] {
                continue;
            }

            self.term.state.dirty[y] = false;
            let line = self.term.state.line[y].as_ptr_range();
            let safe_line = unsafe {
                std::slice::from_raw_parts(line.start, line.end.offset_from(line.start) as usize)
            };

            self.xdrawline(safe_line, x1, y, x2);
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
        if self.term.state.selected(ox, oy) {
            unsafe { (*og).mode.toggle(GlyphAttribute::ATTR_REVERSE) };
        }

        // Redraw the line where cursor was previously.
        // It will restore the ligatures broken by the cursor.

        self.xdrawline(line, 0, oy, len);
    }

    fn xfinishdraw(&self) {
        // TODO:
    }

    fn xximspot(&self, _ocx: usize, _ocy: usize) {
        // TODO:
    }

    fn xdrawline(&mut self, line: &[Glyph], x1: i32, y1: usize, x2: usize) {
        let _i = 0;
        let _x = 0;
        let _ox = 0;
        let _numspecs = 0;

        let _base: Glyph = Glyph::default();
        let _new: Glyph = Glyph::default();

        let glyphs = &line[x1 as usize..x2];
        let glyphs: Vec<TermGlyph> = self.xdrawglyphfontspecs(&glyphs);

        unsafe {
            self.quad_renderer.as_ref().unwrap().with(|| {
                let proj = ortho(self.term.state.pixw as f32, self.term.state.pixh as f32);

                self.text_renderer
                    .as_mut()
                    .unwrap()
                    .draw_glyphs(&glyphs, y1 as i32, x1 as i32, &proj);
            });
        }
    }

    fn xdrawglyphfontspecs(&self, glyphs: &[Glyph]) -> Vec<TermGlyph> {
        glyphs
            .iter()
            .map(|g| {
                let mut fg = g.fg;
                let bg = g.bg;

                if g.mode.intersects(GlyphAttribute::ATTR_BOLD_FAINT) && fg < 7 {
                    fg += 8;
                }

                let mut fg_color = self.term.colors.get_from_glyph_color(fg);
                let mut bg_color = self.term.colors.get_from_glyph_color(bg);

                if g.mode.contains(GlyphAttribute::ATTR_REVERSE) {
                    std::mem::swap(&mut fg_color, &mut bg_color);
                }

                if g.mode.contains(GlyphAttribute::ATTR_BLINK)
                    && self.term.win.mode.contains(WinMode::Blink)
                {
                    fg_color = bg_color;
                }

                if g.mode.contains(GlyphAttribute::ATTR_INVISIBLE) {
                    fg_color = bg_color;
                }

                TermGlyph {
                    char: g.u,
                    fg_color,
                    bg_color,
                    font_style: glyph_to_font_style(g),
                }
            })
            .collect()
    }

    fn xstartimagedraw(&self, _dirty: &[bool], _row: usize) {
        // todo!()
    }

    fn xfinishimagedraw(&self) {
        // todo!()
    }

    /// Arranges for `new_events` to run again after `timeout_ms`, clamping
    /// negative values (st's "block forever") to `MINLATENCY` since we must
    /// keep polling the tty fd ourselves.
    fn schedule(&self, event_loop: &ActiveEventLoop, now: Instant, timeout_ms: f64) {
        let wait_ms = if timeout_ms < 0.0 {
            MINLATENCY as f64
        } else {
            timeout_ms
        };

        event_loop.set_control_flow(ControlFlow::WaitUntil(
            now + Duration::from_secs_f64(wait_ms / 1e3),
        ));
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

        self.term.win.mode.insert(WinMode::Visible);

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
        self.term.win.cw = font_size.width as u32;
        self.term.win.ch = font_size.height as u32;

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
        if ttyread_pending() {
            self.loop_timeout = 0.0;
        }

        // TODO:
        // /* Decrease the timeout if there are active animations. */
        // if (graphics_next_redraw_delay != INT_MAX && IS_SET(MODE_VISIBLE)) {
        // 	timeout = timeout < 0 ? graphics_next_redraw_delay : MIN(timeout, graphics_next_redraw_delay);
        // }

        // Unlike st, winit owns the X/Wayland connection itself; there's no
        // xfd to add to this select, so (unlike st) we can never block here
        // indefinitely or we'd freeze resize/keyboard handling until the next
        // tty byte. Bound the wait the same way an idle st would eventually
        // wake up on XPending().
        let timeout = if self.loop_timeout < 0.0 {
            MINLATENCY as f64
        } else {
            self.loop_timeout
        };

        let secs = (timeout / 1e3).trunc();
        let seltv = libc::timespec {
            tv_sec: secs as libc::time_t,
            tv_nsec: ((timeout - secs * 1e3) * 1e6) as libc::c_long,
        };

        let ttyin = unsafe {
            libc::FD_ZERO(&mut self.rfd);
            libc::FD_SET(self.ttyfd, &mut self.rfd);

            let ready = libc::pselect(
                self.ttyfd + 1,
                &mut self.rfd,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &seltv,
                std::ptr::null(),
            );

            if ready < 0 {
                if *libc::__errno_location() != libc::EINTR {
                    panic!("pselect failed: {}", std::io::Error::last_os_error());
                }
                // interrupted by a signal: retry immediately, like C's `continue`
                event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now()));
                return;
            }

            libc::FD_ISSET(self.ttyfd, &mut self.rfd) || ttyread_pending()
        };

        let now = Instant::now();

        if ttyin {
            self.term.ttyread();
        }

        // `xev` in st is set whenever any X event was pumped this
        // iteration; here that's whatever window_event saw since we last
        // looked.
        let xev = std::mem::replace(&mut self.xev_pending, false);

        // To reduce flicker and tearing, when new content or an event
        // triggers drawing, first wait a bit to make sure we got everything;
        // if nothing new arrives, draw. Retry with shorter and shorter
        // periods, drawing even without idle after maxlatency ms.
        if ttyin || xev {
            if !self.drawing {
                self.trigger = now;
                self.drawing = true;

                if self.term.win.mode.contains(WinMode::Blink) {
                    self.term.win.mode.toggle(WinMode::Blink);
                }

                self.last_blink = now;
            }

            let elapsed_ms = (now - self.trigger).as_secs_f64() * 1e3;
            self.loop_timeout =
                (MAXLATENCY as f64 - elapsed_ms) / MAXLATENCY as f64 * MINLATENCY as f64;

            if self.loop_timeout > 0.0 {
                // we have time, try to find idle
                self.schedule(event_loop, now, self.loop_timeout);
                return;
            }
        }

        // on synchronized-update draw-suspension: don't reset `drawing` so we
        // draw ASAP once we can.
        // NOTE: `tinsync`'s elapsed-time check isn't
        // ported yet (no `sutv`), and DECSET 2026 (`tsetmode`) is still a
        // stub, so SU never actually becomes nonzero today.
        if unsafe { SU } != 0 {
            self.loop_timeout = MINLATENCY as f64;
            self.schedule(event_loop, now, self.loop_timeout);
            return;
        }

        // idle detected or maxlatency exhausted -> draw
        self.loop_timeout = -1.0;

        if config::BLINK_TIMEOUT > 0 && self.term.state.tattrset(GlyphAttribute::ATTR_BLINK) {
            let elapsed_ms = (now - self.last_blink).as_secs_f64() * 1e3;
            if elapsed_ms >= config::BLINK_TIMEOUT as f64 {
                self.term.win.mode.toggle(WinMode::Blink);
                self.last_blink = now;
                self.term.state.tsetdirtattr(GlyphAttribute::ATTR_BLINK);

                self.loop_timeout = config::BLINK_TIMEOUT as f64;
            }
        }

        self.draw();
        self.drawing = false;

        if let Some(window) = self.app_state.as_ref().map(|s| &s.window) {
            window.request_redraw();
        }

        self.schedule(event_loop, now, self.loop_timeout);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _id: WindowId,
        event: winit::event::WindowEvent,
    ) {
        // Like st's `xev`: any real window event should re-arm the drawing
        // debounce in `new_events`. RedrawRequested is excluded since we
        // generate it ourselves after drawing, not an external event.
        if !matches!(event, WindowEvent::RedrawRequested) {
            self.xev_pending = true;
        }

        match event {
            WindowEvent::ModifiersChanged(modifiers) => {
                self.keyboard_modifiers = modifiers.state();
                println!("Modifiers changed: {:?}", self.keyboard_modifiers);
            }
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

                // let duration = start.elapsed();
                // println!("Redrawn in {} ms", duration.as_millis());
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

fn ttyread_pending() -> bool {
    unsafe { TWRITE_ABORTED }
}

fn glyph_to_font_style(g: &Glyph) -> FontStyle {
    if g.mode.intersects(GlyphAttribute::ATTR_BOLD_FAINT)
        && g.mode.contains(GlyphAttribute::ATTR_ITALIC)
    {
        FontStyle::ItalicBold
    } else if g.mode.contains(GlyphAttribute::ATTR_ITALIC) {
        FontStyle::Italic
    } else if g.mode.intersects(GlyphAttribute::ATTR_BOLD_FAINT) {
        FontStyle::Bold
    } else {
        FontStyle::Regular
    }
}
