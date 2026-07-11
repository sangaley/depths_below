use bevy::prelude::*;

// ============================================================================
// FLOATING WINDOW FRAMEWORK
// Draggable, closeable, z-orderable, collapsible windows.
// Any game UI content can live inside a FloatingWindow.
// ============================================================================

/// Marker for the root entity of a floating window
#[derive(Component)]
pub struct FloatingWindow {
    pub id: String,
    pub title: String,
    pub is_collapsed: bool,
    pub z_order: u32,
    pub is_dragging: bool,
    pub drag_offset: Vec2,
    pub min_size: Vec2,
}

/// Marker for the title bar (drag handle)
#[derive(Component)]
pub struct WindowTitleBar {
    pub window_id: String,
}

/// Marker for the close button
#[derive(Component)]
pub struct WindowCloseButton {
    pub window_id: String,
}

/// Marker for the collapse/expand button
#[derive(Component)]
pub struct WindowCollapseButton {
    pub window_id: String,
}

/// Marker for the content area of a floating window
#[derive(Component)]
pub struct WindowContent {
    pub window_id: String,
}

/// Global z-order counter for bringing windows to front
#[derive(Resource, Default)]
pub struct WindowZCounter {
    pub next_z: u32,
}

/// Node constants for consistent window appearance
pub struct WindowStyle;

impl WindowStyle {
    pub const TITLE_BAR_HEIGHT: f32 = 28.0;
    pub const BORDER_WIDTH: f32 = 1.0;
    pub const PADDING: f32 = 8.0;
    pub const BG_COLOR: Color = Color::srgba(0.08, 0.10, 0.16, 0.95);
    pub const TITLE_BG: Color = Color::srgba(0.12, 0.15, 0.22, 1.0);
    pub const TITLE_BG_HOVER: Color = Color::srgba(0.16, 0.20, 0.28, 1.0);
    pub const BORDER_COLOR: Color = Color::srgba(0.25, 0.30, 0.40, 0.8);
    pub const CLOSE_COLOR: Color = Color::srgba(0.8, 0.3, 0.3, 1.0);
    pub const CLOSE_HOVER: Color = Color::srgba(1.0, 0.4, 0.4, 1.0);
    pub const COLLAPSE_COLOR: Color = Color::srgba(0.6, 0.6, 0.3, 1.0);
    pub const TEXT_COLOR: Color = Color::srgba(0.8, 0.85, 0.9, 1.0);
    pub const TEXT_DIM: Color = Color::srgba(0.5, 0.55, 0.6, 1.0);
}

/// Spawn a floating window. Returns the content entity where you add your UI children.
///
/// Usage:
/// ```
/// let content = spawn_floating_window(
///     &mut commands,
///     "my_window",
///     "Window Title",
///     Vec2::new(300.0, 200.0),  // size
///     Vec2::new(100.0, 100.0),  // position (top-left)
/// );
/// // Then add children to `content`
/// commands.entity(content).with_children(|parent| { ... });
/// ```
pub fn spawn_floating_window(
    commands: &mut Commands,
    id: &str,
    title: &str,
    size: Vec2,
    position: Vec2,
) -> Entity {
    let id_str = id.to_string();
    let title_str = title.to_string();

    // Root window container
    let window_entity = commands.spawn((
        (Node {
                position_type: PositionType::Absolute,
                left: Val::Px(position.x),
                top: Val::Px(position.y),
                width: Val::Px(size.x),
                min_width: Val::Px(150.0),
                flex_direction: FlexDirection::Column,
                ..default()
            }, BackgroundColor(WindowStyle::BG_COLOR), ZIndex(10)),
        FloatingWindow {
            id: id_str.clone(),
            title: title_str.clone(),
            is_collapsed: false,
            z_order: 0,
            is_dragging: false,
            drag_offset: Vec2::ZERO,
            min_size: Vec2::new(150.0, 80.0),
        },
        Interaction::None,
    )).id();

    // Title bar
    let title_bar = commands.spawn((
        (Node {
                width: Val::Percent(100.0),
                height: Val::Px(WindowStyle::TITLE_BAR_HEIGHT),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                padding: UiRect::horizontal(Val::Px(6.0)),
                ..default()
            }, BackgroundColor(WindowStyle::TITLE_BG)),
        WindowTitleBar { window_id: id_str.clone() },
        Interaction::None,
    )).id();

    // Title text
    let title_text = commands.spawn(
        (Text::new(title), TextFont { font_size: FontSize::Px(13.0), ..default() }, TextColor(WindowStyle::TEXT_COLOR)),
    ).id();

    // Button container (collapse + close)
    let button_container = commands.spawn(
        (Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(4.0),
                ..default()
            }),
    ).id();

    // Collapse button (—)
    let collapse_btn = commands.spawn((
        (Node {
                width: Val::Px(20.0),
                height: Val::Px(18.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            }, BackgroundColor(Color::NONE)),
        WindowCollapseButton { window_id: id_str.clone() },
    )).id();

    let collapse_text = commands.spawn(
        (Text::new("—"), TextFont { font_size: FontSize::Px(14.0), ..default() }, TextColor(WindowStyle::COLLAPSE_COLOR)),
    ).id();

    // Close button (×)
    let close_btn = commands.spawn((
        (Node {
                width: Val::Px(20.0),
                height: Val::Px(18.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            }, BackgroundColor(Color::NONE)),
        WindowCloseButton { window_id: id_str.clone() },
    )).id();

    let close_text = commands.spawn(
        (Text::new("×"), TextFont { font_size: FontSize::Px(16.0), ..default() }, TextColor(WindowStyle::CLOSE_COLOR)),
    ).id();

    // Content area
    let content_entity = commands.spawn((
        (Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(WindowStyle::PADDING)),
                row_gap: Val::Px(4.0),
                flex_grow: 1.0,
                overflow: Overflow::clip_y(),
                ..default()
            }),
        WindowContent { window_id: id_str },
    )).id();

    // Border (bottom line for visual separation between title and content)
    let border = commands.spawn(
        (Node {
                width: Val::Percent(100.0),
                height: Val::Px(1.0),
                ..default()
            }, BackgroundColor(WindowStyle::BORDER_COLOR)),
    ).id();

    // Build hierarchy
    commands.entity(collapse_btn).add_child(collapse_text);
    commands.entity(close_btn).add_child(close_text);
    commands.entity(button_container).add_children(&[collapse_btn, close_btn]);
    commands.entity(title_bar).add_children(&[title_text, button_container]);
    commands.entity(window_entity).add_children(&[title_bar, border, content_entity]);

    content_entity
}

/// System: handle window dragging via title bar interaction
pub fn window_drag_system(
    mut windows: Query<(&mut FloatingWindow, &mut Node, &mut ZIndex)>,
    title_bars: Query<(&WindowTitleBar, &Interaction)>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    cursor_query: Query<&Window>,
    mut z_counter: ResMut<WindowZCounter>,
) {
    let Some(cursor_pos) = cursor_query.single().ok()
        .and_then(|w| w.cursor_position())
    else {
        return;
    };

    // Start drag on title bar click
    for (title_bar, interaction) in title_bars.iter() {
        if *interaction == Interaction::Pressed {
            for (mut window, style, mut z_index) in windows.iter_mut() {
                if window.id == title_bar.window_id {
                    if !window.is_dragging && mouse_button.just_pressed(MouseButton::Left) {
                        window.is_dragging = true;
                        let current_x = match style.left {
                            Val::Px(x) => x,
                            _ => 0.0,
                        };
                        let current_y = match style.top {
                            Val::Px(y) => y,
                            _ => 0.0,
                        };
                        window.drag_offset = Vec2::new(
                            cursor_pos.x - current_x,
                            cursor_pos.y - current_y,
                        );
                        // Bring to front
                        z_counter.next_z += 1;
                        window.z_order = z_counter.next_z;
                        *z_index = ZIndex(10 + z_counter.next_z as i32);
                    }
                }
            }
        }
    }

    // Continue drag
    if mouse_button.pressed(MouseButton::Left) {
        for (window, mut style, _) in windows.iter_mut() {
            if window.is_dragging {
                let new_x = (cursor_pos.x - window.drag_offset.x).max(0.0);
                let new_y = (cursor_pos.y - window.drag_offset.y).max(0.0);
                style.left = Val::Px(new_x);
                style.top = Val::Px(new_y);
            }
        }
    }

    // Stop drag on release
    if mouse_button.just_released(MouseButton::Left) {
        for (mut window, _, _) in windows.iter_mut() {
            window.is_dragging = false;
        }
    }
}

/// System: handle close button clicks
pub fn window_close_system(
    mut commands: Commands,
    close_buttons: Query<(&WindowCloseButton, &Interaction), Changed<Interaction>>,
    windows: Query<(Entity, &FloatingWindow)>,
) {
    for (close_btn, interaction) in close_buttons.iter() {
        if *interaction == Interaction::Pressed {
            for (entity, window) in windows.iter() {
                if window.id == close_btn.window_id {
                    commands.entity(entity).despawn();
                }
            }
        }
    }
}

/// System: handle collapse button clicks
pub fn window_collapse_system(
    collapse_buttons: Query<(&WindowCollapseButton, &Interaction), Changed<Interaction>>,
    mut windows: Query<&mut FloatingWindow>,
    mut content_query: Query<(&WindowContent, &mut Node)>,
    _border_query: Query<&mut Node, (Without<WindowContent>, Without<FloatingWindow>)>,
) {
    for (collapse_btn, interaction) in collapse_buttons.iter() {
        if *interaction == Interaction::Pressed {
            for mut window in windows.iter_mut() {
                if window.id == collapse_btn.window_id {
                    window.is_collapsed = !window.is_collapsed;

                    // Toggle content visibility
                    for (content, mut style) in content_query.iter_mut() {
                        if content.window_id == window.id {
                            style.display = if window.is_collapsed {
                                Display::None
                            } else {
                                Display::Flex
                            };
                        }
                    }
                }
            }
        }
    }
}

/// System: visual feedback on close/collapse button hover
pub fn window_button_hover_system(
    mut close_buttons: Query<
        (&Interaction, &mut BackgroundColor),
        (With<WindowCloseButton>, Changed<Interaction>),
    >,
    mut collapse_buttons: Query<
        (&Interaction, &mut BackgroundColor),
        (With<WindowCollapseButton>, Without<WindowCloseButton>, Changed<Interaction>),
    >,
) {
    for (interaction, mut bg) in close_buttons.iter_mut() {
        *bg = match interaction {
            Interaction::Hovered => Color::srgba(0.8, 0.2, 0.2, 0.3).into(),
            Interaction::Pressed => Color::srgba(1.0, 0.3, 0.3, 0.5).into(),
            Interaction::None => Color::NONE.into(),
        };
    }
    for (interaction, mut bg) in collapse_buttons.iter_mut() {
        *bg = match interaction {
            Interaction::Hovered => Color::srgba(0.6, 0.6, 0.2, 0.3).into(),
            Interaction::Pressed => Color::srgba(0.8, 0.8, 0.3, 0.5).into(),
            Interaction::None => Color::NONE.into(),
        };
    }
}

/// Helper: spawn a labeled row inside a window content area (label: value)
pub fn spawn_window_row(
    commands: &mut Commands,
    parent: Entity,
    label: &str,
    value: &str,
    label_color: Color,
    value_color: Color,
) -> Entity {
    let row = commands.spawn(
        (Node {
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::SpaceBetween,
                width: Val::Percent(100.0),
                ..default()
            }),
    ).id();

    let label_text = commands.spawn(
        (Text::new(label), TextFont { font_size: FontSize::Px(12.0), ..default() }, TextColor(label_color)),
    ).id();

    let value_text = commands.spawn(
        (Text::new(value), TextFont { font_size: FontSize::Px(12.0), ..default() }, TextColor(value_color)),
    ).id();

    commands.entity(row).add_children(&[label_text, value_text]);
    commands.entity(parent).add_child(row);
    row
}

/// Helper: spawn a section header inside a window
pub fn spawn_window_section(
    commands: &mut Commands,
    parent: Entity,
    title: &str,
) {
    let header = commands.spawn(
        (Node {
                width: Val::Percent(100.0),
                padding: UiRect::new(Val::Px(0.0), Val::Px(0.0), Val::Px(4.0), Val::Px(2.0)),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            }, BorderColor::all(WindowStyle::BORDER_COLOR)),
    ).id();

    let text = commands.spawn(
        (Text::new(title), TextFont { font_size: FontSize::Px(11.0), ..default() }, TextColor(WindowStyle::TEXT_DIM)),
    ).id();

    commands.entity(header).add_child(text);
    commands.entity(parent).add_child(header);
}
