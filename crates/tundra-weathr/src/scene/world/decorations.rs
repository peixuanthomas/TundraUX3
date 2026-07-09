use crate::assets::WorldSceneAssets;
use crate::render::TerminalRenderer;
use crate::scene::world::style::WorldSceneStyle;
use std::io;

pub struct Decorations {
    tree: Vec<String>,
    fence: Vec<String>,
    mailbox: Vec<String>,
    pine_tree: Vec<String>,
}

pub struct DecorationLayout {
    pub horizon_y: u16,
    pub house_x: u16,
    pub house_width: u16,
    pub width: u16,
}

impl Decorations {
    pub fn new(assets: &WorldSceneAssets) -> Self {
        Self {
            tree: assets.tree.clone(),
            fence: assets.fence.clone(),
            mailbox: assets.mailbox.clone(),
            pine_tree: assets.pine_tree.clone(),
        }
    }

    pub fn render(
        &self,
        renderer: &mut TerminalRenderer,
        layout: &DecorationLayout,
        style: &WorldSceneStyle,
    ) -> io::Result<()> {
        self.render_tree(renderer, layout, style)?;
        self.render_fence(renderer, layout, style)?;
        self.render_mailbox(renderer, layout, style)?;

        if layout.width > 120 {
            self.render_pine_tree(renderer, layout, style)?;
        }

        Ok(())
    }

    fn render_tree(
        &self,
        renderer: &mut TerminalRenderer,
        layout: &DecorationLayout,
        style: &WorldSceneStyle,
    ) -> io::Result<()> {
        let tree_x = layout.house_x.saturating_sub(20);
        if tree_x == 0 {
            return Ok(());
        }
        let line_count = self.tree.len() as u16;
        let tree_y = layout.horizon_y.saturating_sub(line_count);
        render_art(renderer, &self.tree, tree_x, tree_y, style.tree_foliage)
    }

    fn render_fence(
        &self,
        renderer: &mut TerminalRenderer,
        layout: &DecorationLayout,
        style: &WorldSceneStyle,
    ) -> io::Result<()> {
        let fence_x = layout.house_x + layout.house_width + 2;
        if fence_x >= layout.width {
            return Ok(());
        }
        let line_count = self.fence.len() as u16;
        let fence_y = layout.horizon_y.saturating_sub(line_count);
        render_art(renderer, &self.fence, fence_x, fence_y, style.fence)
    }

    fn render_mailbox(
        &self,
        renderer: &mut TerminalRenderer,
        layout: &DecorationLayout,
        style: &WorldSceneStyle,
    ) -> io::Result<()> {
        let tree_x = layout.house_x.saturating_sub(20);
        let Some(mailbox_x) = tree_x.checked_sub(10) else {
            return Ok(());
        };
        if mailbox_x >= layout.width {
            return Ok(());
        }
        let line_count = self.mailbox.len() as u16;
        let mailbox_y = layout.horizon_y.saturating_sub(line_count);
        render_art(renderer, &self.mailbox, mailbox_x, mailbox_y, style.mailbox)
    }

    fn render_pine_tree(
        &self,
        renderer: &mut TerminalRenderer,
        layout: &DecorationLayout,
        style: &WorldSceneStyle,
    ) -> io::Result<()> {
        let pine_x = layout.house_x + layout.house_width + 18;
        if pine_x + 10 >= layout.width {
            return Ok(());
        }
        let line_count = self.pine_tree.len() as u16;
        let pine_y = layout.horizon_y.saturating_sub(line_count);
        render_art(
            renderer,
            &self.pine_tree,
            pine_x,
            pine_y,
            style.tree_foliage,
        )
    }
}

fn render_art(
    renderer: &mut TerminalRenderer,
    art: &[String],
    x: u16,
    y: u16,
    color: crossterm::style::Color,
) -> io::Result<()> {
    for (i, line) in art.iter().enumerate() {
        for (j, ch) in line.chars().enumerate() {
            if ch != ' ' {
                renderer.render_char(x + j as u16, y + i as u16, ch, color)?;
            }
        }
    }
    Ok(())
}
