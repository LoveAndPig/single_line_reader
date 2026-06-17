use crate::config::AppConfig;
use crate::state::{AppState, SharedState};
use egui::{Color32, Context, ViewportCommand};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

pub struct ReaderApp {
    state: SharedState,
    running: Arc<AtomicBool>,
    window_visible: Arc<AtomicBool>,
    // 缓存配置值，避免每帧加锁读取
    cached_font_size: u32,
    cached_bg: Color32,
    cached_fg: Color32,
    cached_always_on_top: bool,
    cached_shortcuts: crate::config::ShortcutConfig,
}

impl ReaderApp {
    pub fn new(
        state: SharedState,
        running: Arc<AtomicBool>,
        window_visible: Arc<AtomicBool>,
    ) -> Self {
        let cfg = crate::config::AppConfig::global().lock().unwrap();
        let bg = AppConfig::parse_color(&cfg.style.bg_color);
        let fg = AppConfig::parse_color(&cfg.style.font_color);
        let result = Self {
            state,
            running,
            window_visible,
            cached_font_size: cfg.style.font_size,
            cached_bg: Color32::from_rgb(
                ((bg >> 16) & 0xFF) as u8,
                ((bg >> 8) & 0xFF) as u8,
                (bg & 0xFF) as u8,
            ),
            cached_fg: Color32::from_rgb(
                ((fg >> 16) & 0xFF) as u8,
                ((fg >> 8) & 0xFF) as u8,
                (fg & 0xFF) as u8,
            ),
            cached_always_on_top: cfg.always_on_top,
            cached_shortcuts: cfg.shortcuts.clone(),
        };
        result
    }

    /// 从全局配置刷新所有缓存值
    fn refresh_config_cache(&mut self) {
        let cfg = crate::config::AppConfig::global().lock().unwrap();
        let bg = AppConfig::parse_color(&cfg.style.bg_color);
        let fg = AppConfig::parse_color(&cfg.style.font_color);
        self.cached_font_size = cfg.style.font_size;
        self.cached_bg = Color32::from_rgb(
            ((bg >> 16) & 0xFF) as u8,
            ((bg >> 8) & 0xFF) as u8,
            (bg & 0xFF) as u8,
        );
        self.cached_fg = Color32::from_rgb(
            ((fg >> 16) & 0xFF) as u8,
            ((fg >> 8) & 0xFF) as u8,
            (fg & 0xFF) as u8,
        );
        self.cached_always_on_top = cfg.always_on_top;
        self.cached_shortcuts = cfg.shortcuts.clone();
    }

    fn with_state<R>(&self, f: impl FnOnce(&AppState) -> R) -> R {
        let s = self.state.lock().unwrap();
        f(&s)
    }

    fn with_state_mut<R>(&self, f: impl FnOnce(&mut AppState) -> R) -> R {
        let mut s = self.state.lock().unwrap();
        f(&mut s)
    }
}

impl eframe::App for ReaderApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.refresh_config_cache();

        // 检查运行标志
        if !self.running.load(Ordering::SeqCst) {
            ctx.send_viewport_cmd(ViewportCommand::Close);
            return;
        }

        // 处理屏幕取色
        let picking = self.with_state(|s| s.color_picking);
        if let Some(target) = picking {
            // 清除取色状态（避免重复触发）
            self.with_state_mut(|s| s.color_picking = None);

            let state_clone = self.state.clone();
            std::thread::spawn(move || {
                let hwnd = crate::color_picker::find_viewport_hwnd();
                if let Some([r, g, b]) = crate::color_picker::pick_screen_color(hwnd) {
                    let mut s = state_clone.lock().unwrap();
                    let color = [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0];
                    match target {
                        0 => s.tmp_bg_color = color,
                        1 => s.tmp_font_color = color,
                        _ => {}
                    }
                    // 重新打开样式对话框
                    s.show_style_dialog = true;
                }
            });
        }

        // 同步托盘恢复的窗口可见性
        if self.window_visible.load(Ordering::SeqCst) {
            let is_visible = self.with_state(|s| s.is_visible);
            if !is_visible {
                ctx.send_viewport_cmd(ViewportCommand::Visible(true));
                self.with_state_mut(|s| s.is_visible = true);
            }
        }

        // 处理键盘输入
        self.handle_keyboard(&*ctx_read_only(ctx));

        // 主面板 - 单行文字显示（使用缓存值，无需每帧加锁）
        let bg = self.cached_bg;
        let fg = self.cached_fg;

        let frame = egui::Frame::central_panel(&ctx.style())
            .fill(bg)
            .inner_margin(egui::Margin::symmetric(4, 0));

        egui::CentralPanel::default()
            .frame(frame)
            .show(ctx, |ui| {
                let response = ui.interact(
                    ui.max_rect(),
                    ui.next_auto_id(),
                    egui::Sense::click_and_drag(),
                );

                // 拖拽移动窗口
                if response.dragged() {
                    ctx.send_viewport_cmd(ViewportCommand::StartDrag);
                }

                // 右键点击：使用 GetCursorPos 获取屏幕绝对坐标打开菜单
                if response.secondary_clicked() {
                    let mut pt = POINT::default();
                    unsafe { let _ = GetCursorPos(&mut pt); }
                    self.with_state_mut(|s| {
                        s.show_context_menu = true;
                        s.menu_position = (pt.x as f32, pt.y as f32);
                    });
                }

                // 左键点击：如果是图片行则显示图片，否则关闭菜单
                if response.clicked() {
                    let is_image_line = {
                        let s = self.state.lock().unwrap();
                        s.get_current_image().is_some()
                    };
                    if is_image_line {
                        self.with_state_mut(|s| {
                            s.show_context_menu = false;
                            s.show_image_dialog = true;
                        });
                    } else {
                        self.with_state_mut(|s| s.show_context_menu = false);
                    }
                }

                // 双击隐藏到托盘
                if response.double_clicked() {
                    ctx.send_viewport_cmd(ViewportCommand::Visible(false));
                    self.with_state_mut(|s| s.is_visible = false);
                    self.window_visible.store(false, Ordering::SeqCst);
                }

                // 渲染单行文字
                let font_size = self.cached_font_size;
                let (text, scroll_offset, is_image_line) = {
                    let s = self.state.lock().unwrap();
                    (
                        s.current_line_text(),
                        s.scroll_offset,
                        s.get_current_image().is_some(),
                    )
                };

                let available = ui.available_size();
                let galley = ui.painter().layout(
                    text.clone(),
                    egui::FontId::proportional(font_size as f32),
                    if is_image_line {
                        // 图片行使用醒目的蓝色+下划线样式
                        Color32::from_rgb(100, 180, 255)
                    } else {
                        fg
                    },
                    f32::INFINITY,
                );

                let text_width = galley.size().x;
                let text_height = galley.size().y;

                // 计算最大滚动偏移量：文字宽度超过可用宽度时，允许滚动超出部分
                let max_scroll = (text_width - available.x).max(0.0);

                // 将 max_scroll_offset 存回 state，供 scroll_left/scroll_right 使用
                {
                    let mut s = self.state.lock().unwrap();
                    let clamped_offset = scroll_offset.clamp(0.0, max_scroll);
                    s.scroll_offset = clamped_offset;
                    s.max_scroll_offset = max_scroll;
                }

                let y_offset = (available.y - text_height) / 2.0;
                if y_offset > 0.0 {
                    ui.add_space(y_offset);
                }

                // 裁剪矩形
                let clip_rect = ui.max_rect();
                ui.painter().with_clip_rect(clip_rect);

                let pos = egui::pos2(
                    ui.max_rect().left() - {
                        let s = self.state.lock().unwrap();
                        s.scroll_offset
                    },
                    ui.next_widget_position().y,
                );
                ui.painter().galley(pos, galley, if is_image_line {
                    Color32::from_rgb(100, 180, 255)
                } else {
                    fg
                });

                // 图片行添加下划线提示可点击
                if is_image_line {
                    let underline_y = pos.y + text_height + 1.0;
                    let underline_p1 = egui::pos2(pos.x, underline_y);
                    let underline_p2 = egui::pos2((pos.x + text_width).min(ui.max_rect().right()), underline_y);
                    ui.painter().line_segment(
                        [underline_p1, underline_p2],
                        egui::Stroke::new(1.5, Color32::from_rgb(100, 180, 255)),
                    );
                }
            });

        // ---- 独立 viewport 窗口 ----

        // 右键菜单（独立 viewport）
        if self.with_state(|s| s.show_context_menu) {
            let state = self.state.clone();
            let running = self.running.clone();
            let cached_top = self.cached_always_on_top;
            let (px, py) = self.with_state(|s| s.menu_position);

            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("context_menu"),
                egui::ViewportBuilder::default()
                    .with_decorations(false)
                    .with_title("")
                    .with_inner_size([180.0, 340.0])
                    .with_position(egui::pos2(px, py))
                    .with_resizable(false)
                    .with_maximize_button(false)
                    .with_minimize_button(false)
                    .with_always_on_top(),
                move |vctx, _class| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::popup(&vctx.style()))
                        .show(vctx, |ui| {
                            ui.set_min_width(160.0);

                            // 选择文件 → 本地文件
                            if ui.button("选择本地文件").clicked() {
                                let state_inner = state.clone();
                                std::thread::spawn(move || {
                                    if let Some(path) = rfd::FileDialog::new()
                                        .add_filter("文档", &["txt", "epub", "docx", "doc"])
                                        .pick_file()
                                    {
                                        let mut s = state_inner.lock().unwrap();
                                        s.load_file(&path);
                                    }
                                });
                                state.lock().unwrap().show_context_menu = false;
                                vctx.send_viewport_cmd(ViewportCommand::Close);
                            }

                            // 历史记录
                            if ui.button("历史记录").clicked() {
                                state.lock().unwrap().show_context_menu = false;
                                state.lock().unwrap().show_history_dialog = true;
                                vctx.send_viewport_cmd(ViewportCommand::Close);
                            }

                            ui.separator();

                            // 置顶
                            let top_label = if cached_top { "取消置顶" } else { "置顶" };
                            if ui.button(top_label).clicked() {
                                let mut cfg = crate::config::AppConfig::global().lock().unwrap();
                                cfg.always_on_top = !cfg.always_on_top;
                                let _ = cfg.save();
                                drop(cfg);
                                state.lock().unwrap().show_context_menu = false;
                                vctx.send_viewport_cmd(ViewportCommand::Close);
                            }

                            ui.separator();

                            // 章节跳转
                            let has_chapters = !state.lock().unwrap().chapters.is_empty();
                            if has_chapters {
                                if ui.button("跳转到章节").clicked() {
                                    state.lock().unwrap().show_context_menu = false;
                                    state.lock().unwrap().show_chapter_dialog = true;
                                    vctx.send_viewport_cmd(ViewportCommand::Close);
                                }
                            } else {
                                ui.add_enabled(false, egui::Button::new("跳转到章节(无章节)"));
                            }

                            // 样式设置
                            if ui.button("样式设置").clicked() {
                                let cfg = crate::config::AppConfig::global().lock().unwrap();
                                let bg = AppConfig::parse_color(&cfg.style.bg_color);
                                let fg = AppConfig::parse_color(&cfg.style.font_color);
                                let font = cfg.style.font.clone();
                                let size = cfg.style.font_size;
                                drop(cfg); // 释放 Config 锁
                                let mut s = state.lock().unwrap();
                                s.show_context_menu = false;
                                s.tmp_bg_color = [
                                    ((bg >> 16) & 0xFF) as f32 / 255.0,
                                    ((bg >> 8) & 0xFF) as f32 / 255.0,
                                    (bg & 0xFF) as f32 / 255.0,
                                ];
                                s.tmp_font_color = [
                                    ((fg >> 16) & 0xFF) as f32 / 255.0,
                                    ((fg >> 8) & 0xFF) as f32 / 255.0,
                                    (fg & 0xFF) as f32 / 255.0,
                                ];
                                s.tmp_font_name = font;
                                s.tmp_font_size = size;
                                s.show_style_dialog = true;
                                vctx.send_viewport_cmd(ViewportCommand::Close);
                            }

                            // 快捷键设置
                            if ui.button("快捷键设置").clicked() {
                                state.lock().unwrap().show_context_menu = false;
                                state.lock().unwrap().show_shortcut_dialog = true;
                                vctx.send_viewport_cmd(ViewportCommand::Close);
                            }

                            ui.separator();

                            // 退出
                            if ui.button("退出").clicked() {
                                let s = state.lock().unwrap();
                                s.save_current_history();
                                running.store(false, Ordering::SeqCst);
                                vctx.send_viewport_cmd(ViewportCommand::Close);
                            }
                        });
                },
            );
        }

        // 样式设置对话框（独立 viewport）
        if self.with_state(|s| s.show_style_dialog) {
            let state = self.state.clone();
            let ctx_clone = ctx.clone();

            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("style_dialog"),
                egui::ViewportBuilder::default()
                    .with_title("样式设置")
                    .with_inner_size([380.0, 250.0])
                    .with_resizable(false)
                    .with_maximize_button(false)
                    .with_minimize_button(false),
                move |vctx, _class| {
                    let should_close = render_style_dialog(vctx, &state);
                    if should_close {
                        ctx_clone.request_repaint();
                    }
                },
            );
        }

        // 快捷键设置对话框（独立 viewport）
        if self.with_state(|s| s.show_shortcut_dialog) {
            let state = self.state.clone();

            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("shortcut_dialog"),
                egui::ViewportBuilder::default()
                    .with_title("快捷键设置")
                    .with_inner_size([380.0, 200.0])
                    .with_resizable(false)
                    .with_maximize_button(false)
                    .with_minimize_button(false),
                move |vctx, _class| {
                    render_shortcut_dialog(vctx, &state);
                },
            );
        }

        // 章节列表对话框（独立 viewport）
        if self.with_state(|s| s.show_chapter_dialog) {
            let state = self.state.clone();

            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("chapter_dialog"),
                egui::ViewportBuilder::default()
                    .with_title("章节列表")
                    .with_inner_size([300.0, 400.0])
                    .with_resizable(true),
                move |vctx, _class| {
                    render_chapter_dialog(vctx, &state);
                },
            );
        }

        // 图片显示对话框（独立 viewport）
        if self.with_state(|s| s.show_image_dialog) {
            let has_image = {
                let s = self.state.lock().unwrap();
                s.show_image_dialog
                    && s.get_current_image().is_some()
            };
            if has_image {
                let state = self.state.clone();

                ctx.show_viewport_immediate(
                    egui::ViewportId::from_hash_of("image_dialog"),
                    egui::ViewportBuilder::default()
                        .with_title("图片")
                        .with_inner_size([500.0, 400.0])
                        .with_resizable(true),
                    move |vctx, _class| {
                        render_image_dialog(vctx, &state);
                    },
                );
            } else {
                // 没有可用图片，关闭对话框标记
                self.with_state_mut(|s| s.show_image_dialog = false);
            }
        }

        // 历史记录对话框（独立 viewport）
        if self.with_state(|s| s.show_history_dialog) {
            let state = self.state.clone();

            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of("history_dialog"),
                egui::ViewportBuilder::default()
                    .with_title("阅读历史")
                    .with_inner_size([500.0, 400.0])
                    .with_resizable(true),
                move |vctx, _class| {
                    render_history_dialog(vctx, &state);
                },
            );
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.with_state(|s| s.save_current_history());
        self.running.store(false, Ordering::SeqCst);
    }
}

impl ReaderApp {
    fn handle_keyboard(&mut self, input: &egui::InputState) {
        // 如果正在等待快捷键输入
        let waiting = self.with_state(|s| s.waiting_key.clone());
        if let Some((idx, _)) = waiting {
            for event in &input.events {
                if let egui::Event::Key {
                    key,
                    pressed: true,
                    ..
                } = event
                {
                    let key_str = format!("{:?}", key);
                    let mut cfg = crate::config::AppConfig::global().lock().unwrap();
                    match idx {
                        0 => cfg.shortcuts.prev_line = key_str.clone(),
                        1 => cfg.shortcuts.next_line = key_str.clone(),
                        2 => cfg.shortcuts.scroll_left = key_str.clone(),
                        3 => cfg.shortcuts.scroll_right = key_str.clone(),
                        _ => {}
                    }
                    let _ = cfg.save();
                    let mut s = self.state.lock().unwrap();
                    s.waiting_key = None;
                    return;
                }
            }
            return;
        }

        // 正常键盘处理
        for event in &input.events {
            if let egui::Event::Key {
                key,
                pressed: true,
                ..
            } = event
            {
                let key_name = format!("{:?}", key);
                let shortcuts = &self.cached_shortcuts;
                let mut s = self.state.lock().unwrap();

                if key_name == shortcuts.prev_line {
                    s.prev_line();
                } else if key_name == shortcuts.next_line {
                    s.next_line();
                } else if key_name == shortcuts.scroll_left {
                    s.scroll_left();
                } else if key_name == shortcuts.scroll_right {
                    s.scroll_right();
                }
            }
        }
    }
}

fn ctx_read_only(ctx: &egui::Context) -> std::sync::Arc<egui::InputState> {
    ctx.input(|i| std::sync::Arc::new(i.clone()))
}

// ---- 独立 viewport 渲染函数 ----

fn render_style_dialog(ctx: &egui::Context, state: &SharedState) -> bool {
    // 检测原生关闭按钮
    if ctx.input(|i| i.viewport().close_requested()) {
        state.lock().unwrap().show_style_dialog = false;
        return true;
    }

    let mut should_close = false;

    egui::CentralPanel::default().show(ctx, |ui| {
        // 每帧从状态中读取临时值，保证 widget 状态在帧间不丢失
        let (mut bg, mut fg, mut font_name, mut font_size, fonts) = {
            let s = state.lock().unwrap();
            (
                s.tmp_bg_color,
                s.tmp_font_color,
                s.tmp_font_name.clone(),
                s.tmp_font_size,
                s.fonts.clone(),
            )
        };

        ui.horizontal(|ui| {
            ui.label("背景颜色:");
            ui.color_edit_button_rgb(&mut bg);
            if ui.button("取色").clicked() {
                // 标记正在取色，关闭当前对话框，开始屏幕取色
                state.lock().unwrap().color_picking = Some(0);
                state.lock().unwrap().show_style_dialog = false;
                should_close = true;
            }
        });

        ui.horizontal(|ui| {
            ui.label("字体颜色:");
            ui.color_edit_button_rgb(&mut fg);
            if ui.button("取色").clicked() {
                state.lock().unwrap().color_picking = Some(1);
                state.lock().unwrap().show_style_dialog = false;
                should_close = true;
            }
        });

        ui.horizontal(|ui| {
            ui.label("字体:");
            egui::ComboBox::from_id_salt("font_combo")
                .selected_text(&font_name)
                .show_ui(ui, |ui| {
                    for font in &fonts {
                        if ui.selectable_label(font_name == *font, font).clicked() {
                            font_name = font.clone();
                        }
                    }
                });
        });

        ui.horizontal(|ui| {
            ui.label("字号:");
            if ui.button("-").clicked() && font_size > 8 {
                font_size -= 1;
            }
            ui.add(egui::DragValue::new(&mut font_size).range(8..=72).speed(1));
            if ui.button("+").clicked() && font_size < 72 {
                font_size += 1;
            }
        });

        ui.add_space(8.0);

        ui.horizontal(|ui| {
            if ui.button("确认").clicked() {
                let mut s = state.lock().unwrap();
                s.tmp_bg_color = bg;
                s.tmp_font_color = fg;
                s.tmp_font_name = font_name.clone();
                s.tmp_font_size = font_size;
                s.apply_style();
                s.show_style_dialog = false;
                should_close = true;
            }
            if ui.button("取消").clicked() {
                // 取消时恢复到配置中的原始值（先读 Config，再锁 State）
                {
                    let cfg = crate::config::AppConfig::global().lock().unwrap();
                    let cfg_bg = crate::config::AppConfig::parse_color(&cfg.style.bg_color);
                    let cfg_fg = crate::config::AppConfig::parse_color(&cfg.style.font_color);
                    let cfg_font = cfg.style.font.clone();
                    let cfg_size = cfg.style.font_size;
                    drop(cfg); // 释放 Config 锁

                    let mut s = state.lock().unwrap();
                    s.tmp_bg_color = hex_to_rgb_cfg(cfg_bg);
                    s.tmp_font_color = hex_to_rgb_cfg(cfg_fg);
                    s.tmp_font_name = cfg_font;
                    s.tmp_font_size = cfg_size;
                    s.show_style_dialog = false;
                }
                should_close = true;
            }
        });

        // 每帧将临时值写回状态，确保下次渲染保留用户修改
        {
            let mut s = state.lock().unwrap();
            s.tmp_bg_color = bg;
            s.tmp_font_color = fg;
            s.tmp_font_name = font_name;
            s.tmp_font_size = font_size;
        }
    });

    should_close
}

fn hex_to_rgb_cfg(hex: u32) -> [f32; 3] {
    [
        ((hex >> 16) & 0xFF) as f32 / 255.0,
        ((hex >> 8) & 0xFF) as f32 / 255.0,
        (hex & 0xFF) as f32 / 255.0,
    ]
}

fn render_shortcut_dialog(
    ctx: &egui::Context,
    state: &SharedState,
) {
    // 检测原生关闭按钮
    if ctx.input(|i| i.viewport().close_requested()) {
        state.lock().unwrap().show_shortcut_dialog = false;
        return;
    }

    egui::CentralPanel::default().show(ctx, |ui| {
        let (prev, next, left, right, waiting) = {
            let cfg = crate::config::AppConfig::global().lock().unwrap();
            let prev = cfg.shortcuts.prev_line.clone();
            let next = cfg.shortcuts.next_line.clone();
            let left = cfg.shortcuts.scroll_left.clone();
            let right = cfg.shortcuts.scroll_right.clone();
            drop(cfg); // 释放 Config 锁
            let s = state.lock().unwrap();
            (
                prev,
                next,
                left,
                right,
                s.waiting_key.clone(),
            )
        };

        let shortcuts = [prev, next, left, right];
        let labels = ["上一行:", "下一行:", "向左滚动:", "向右滚动:"];

        for i in 0..4 {
            ui.horizontal(|ui| {
                ui.label(labels[i]);
                let btn_text = if waiting.as_ref().map(|(n, _)| *n == i).unwrap_or(false) {
                    "按下按键...".to_string()
                } else {
                    shortcuts[i].clone()
                };
                if ui.button(&btn_text).clicked() {
                    state.lock().unwrap().waiting_key = Some((i, shortcuts[i].clone()));
                }
            });
        }

        ui.label("点击按钮后按下新按键即可修改快捷键");

        ui.add_space(8.0);
        if ui.button("关闭").clicked() {
            state.lock().unwrap().show_shortcut_dialog = false;
        }

        // 在对话框中处理键盘输入
        let waiting = state.lock().unwrap().waiting_key.clone();
        if let Some((idx, _)) = waiting {
            let input = ctx.input(|i| i.clone());
            for event in &input.events {
                if let egui::Event::Key {
                    key,
                    pressed: true,
                    ..
                } = event
                {
                    let key_str = format!("{:?}", key);
                    let mut cfg = crate::config::AppConfig::global().lock().unwrap();
                    match idx {
                        0 => cfg.shortcuts.prev_line = key_str.clone(),
                        1 => cfg.shortcuts.next_line = key_str.clone(),
                        2 => cfg.shortcuts.scroll_left = key_str.clone(),
                        3 => cfg.shortcuts.scroll_right = key_str.clone(),
                        _ => {}
                    }
                    let _ = cfg.save();
                    state.lock().unwrap().waiting_key = None;
                }
            }
        }
    });
}

fn render_chapter_dialog(ctx: &egui::Context, state: &SharedState) {
    // 检测原生关闭按钮
    if ctx.input(|i| i.viewport().close_requested()) {
        state.lock().unwrap().show_chapter_dialog = false;
        return;
    }

    let chapters = state.lock().unwrap().chapters.clone();

    egui::CentralPanel::default().show(ctx, |ui| {
        ui.label("章节列表");
        ui.separator();

        egui::ScrollArea::vertical().show(ui, |ui| {
            for ch in &chapters {
                let label = format!("第{}行: {}", ch.line_number + 1, ch.title);
                if ui.button(&label).clicked() {
                    let line = ch.line_number;
                    let mut s = state.lock().unwrap();
                    s.goto_line(line);
                    s.show_chapter_dialog = false;
                }
            }
        });

        ui.add_space(8.0);
        if ui.button("关闭").clicked() {
            state.lock().unwrap().show_chapter_dialog = false;
        }
    });
}

fn render_image_dialog(ctx: &egui::Context, state: &SharedState) {
    // 检测原生关闭按钮
    if ctx.input(|i| i.viewport().close_requested()) {
        state.lock().unwrap().show_image_dialog = false;
        return;
    }

    let has_image_data = {
        let s = state.lock().unwrap();
        s.show_image_dialog && s.get_current_image().is_some()
    };

    if !has_image_data {
        state.lock().unwrap().show_image_dialog = false;
        return;
    }

    let (image_data, _format) = {
        let s = state.lock().unwrap();
        let img = s.get_current_image().unwrap();
        (img.data.clone(), img.format.clone())
    };

    egui::CentralPanel::default().show(ctx, |ui| {
        if let Ok(img) = image::load_from_memory(&image_data) {
            let rgba = img.to_rgba8();
            let img_w = rgba.width() as usize;
            let img_h = rgba.height() as usize;
            let pixels = rgba.into_raw();

            let size = [img_w as f32, img_h as f32];
            let available = ui.available_size();
            let scale = (available.x / size[0]).min(available.y / size[1]).min(1.0);
            let display_size = [size[0] * scale, size[1] * scale];

            let texture_handle = ctx.load_texture(
                "image_disp",
                egui::ColorImage::from_rgba_unmultiplied([img_w, img_h], &pixels),
                egui::TextureOptions::default(),
            );

            ui.centered_and_justified(|ui| {
                let sized = egui::load::SizedTexture::new(texture_handle.id(), display_size);
                let img = egui::Image::from_texture(sized)
                    .fit_to_exact_size(egui::vec2(display_size[0], display_size[1]));
                ui.add(img);
            });
        } else {
            ui.label("无法加载图片");
        }
    });
}

fn render_history_dialog(ctx: &egui::Context, state: &SharedState) {
    // 检测原生关闭按钮
    if ctx.input(|i| i.viewport().close_requested()) {
        state.lock().unwrap().show_history_dialog = false;
        return;
    }

    let entries = crate::history::HistoryManager::global().lock().unwrap().get_entries();
    let mut should_close = false;
    let mut jump_to: Option<(String, usize)> = None;

    egui::CentralPanel::default().show(ctx, |ui| {
        // 标题栏 + 关闭按钮（固定在顶部）
        ui.horizontal(|ui| {
            ui.heading("阅读历史");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("关闭").clicked() {
                    should_close = true;
                }
            });
        });
        ui.label("双击条目跳转到对应位置");
        ui.separator();

        if entries.is_empty() {
            ui.add_space(30.0);
            ui.vertical_centered(|ui| {
                ui.label("暂无阅读历史记录");
            });
        } else {
            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    for entry in &entries {
                        let label = format!(
                            "{}  |  第{}行/共{}行  |  {}",
                            entry.file_name,
                            entry.current_line + 1,
                            entry.total_lines,
                            entry.updated_at
                        );
                        let subtitle = &entry.file_path;

                        // 分配固定高度的可点击区域
                        let height = 52.0;
                        let desired_size = egui::vec2(ui.available_width(), height);
                        let (rect, response) = ui.allocate_exact_size(
                            desired_size,
                            egui::Sense::click(),
                        );

                        // 悬停高亮
                        if response.hovered() || response.highlighted() {
                            ui.painter().rect_filled(
                                rect,
                                3.0,
                                egui::Color32::from_white_alpha(25),
                            );
                        }

                        // 在区域内渲染文字
                        if ui.is_rect_visible(rect) {
                            let pos = rect.left_top() + egui::vec2(8.0, 4.0);
                            ui.painter().text(
                                pos,
                                egui::Align2::LEFT_TOP,
                                &label,
                                egui::TextStyle::Button.resolve(&ui.style()),
                                ui.style().visuals.text_color(),
                            );
                            ui.painter().text(
                                pos + egui::vec2(0.0, 22.0),
                                egui::Align2::LEFT_TOP,
                                subtitle,
                                egui::TextStyle::Small.resolve(&ui.style()),
                                egui::Color32::GRAY,
                            );
                        }

                        // 双击跳转
                        if response.double_clicked() {
                            jump_to = Some((entry.file_path.clone(), entry.current_line));
                        }

                        ui.separator();
                    }
                });
        }

        // 底部关闭按钮
        ui.add_space(8.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("关闭").clicked() {
                should_close = true;
            }
        });
    });

    // 处理动作（在 CentralPanel 之后执行，避免借用冲突）
    if should_close {
        state.lock().unwrap().show_history_dialog = false;
    }
    if let Some((path, line)) = jump_to {
        let state_clone = state.clone();
        std::thread::spawn(move || {
            let mut s = state_clone.lock().unwrap();
            let p = std::path::PathBuf::from(&path);
            if s.load_file(&p) {
                s.goto_line(line);
            }
        });
        state.lock().unwrap().show_history_dialog = false;
    }
}