//! Moving the cursor around.

use std::collections::HashSet;

use crate::store::{Msg, MsgStore, Tree};

use super::{Correction, InnerTreeViewState};

#[derive(Debug, Clone, Copy)]
pub enum Cursor<I> {
    Bottom,
    Msg(I),
    Editor {
        coming_from: Option<I>,
        parent: Option<I>,
    },
    Pseudo {
        coming_from: Option<I>,
        parent: Option<I>,
    },
}

impl<I> Cursor<I> {
    pub fn editor(coming_from: Option<I>, parent: Option<I>) -> Self {
        Self::Editor {
            coming_from,
            parent,
        }
    }
}

impl<I: Eq> Cursor<I> {
    pub fn refers_to(&self, id: &I) -> bool {
        if let Self::Msg(own_id) = self {
            own_id == id
        } else {
            false
        }
    }

    pub fn refers_to_last_child_of(&self, id: &I) -> bool {
        if let Self::Editor {
            parent: Some(parent),
            ..
        }
        | Self::Pseudo {
            parent: Some(parent),
            ..
        } = self
        {
            parent == id
        } else {
            false
        }
    }
}

impl<M: Msg, S: MsgStore<M>> InnerTreeViewState<M, S> {
    fn find_parent(tree: &Tree<M>, id: &mut M::Id) -> bool {
        if let Some(parent) = tree.parent(id) {
            *id = parent;
            true
        } else {
            false
        }
    }

    fn find_first_child(folded: &HashSet<M::Id>, tree: &Tree<M>, id: &mut M::Id) -> bool {
        if folded.contains(id) {
            return false;
        }

        if let Some(child) = tree.children(id).and_then(|c| c.first()) {
            *id = child.clone();
            true
        } else {
            false
        }
    }

    fn find_last_child(folded: &HashSet<M::Id>, tree: &Tree<M>, id: &mut M::Id) -> bool {
        if folded.contains(id) {
            return false;
        }

        if let Some(child) = tree.children(id).and_then(|c| c.last()) {
            *id = child.clone();
            true
        } else {
            false
        }
    }

    /// Move to the previous sibling, or don't move if this is not possible.
    ///
    /// Always stays at the same level of indentation.
    async fn find_prev_sibling(
        store: &S,
        tree: &mut Tree<M>,
        id: &mut M::Id,
    ) -> Result<bool, S::Error> {
        let moved = if let Some(prev_sibling) = tree.prev_sibling(id) {
            *id = prev_sibling;
            true
        } else if tree.parent(id).is_none() {
            // We're at the root of our tree, so we need to move to the root of
            // the previous tree.
            if let Some(prev_root_id) = store.prev_root_id(tree.root()).await? {
                *tree = store.tree(&prev_root_id).await?;
                *id = prev_root_id;
                true
            } else {
                false
            }
        } else {
            false
        };
        Ok(moved)
    }

    /// Move to the next sibling, or don't move if this is not possible.
    ///
    /// Always stays at the same level of indentation.
    async fn find_next_sibling(
        store: &S,
        tree: &mut Tree<M>,
        id: &mut M::Id,
    ) -> Result<bool, S::Error> {
        let moved = if let Some(next_sibling) = tree.next_sibling(id) {
            *id = next_sibling;
            true
        } else if tree.parent(id).is_none() {
            // We're at the root of our tree, so we need to move to the root of
            // the next tree.
            if let Some(next_root_id) = store.next_root_id(tree.root()).await? {
                *tree = store.tree(&next_root_id).await?;
                *id = next_root_id;
                true
            } else {
                false
            }
        } else {
            false
        };
        Ok(moved)
    }

    /// Move to the previous message, or don't move if this is not possible.
    async fn find_prev_msg(
        store: &S,
        folded: &HashSet<M::Id>,
        tree: &mut Tree<M>,
        id: &mut M::Id,
    ) -> Result<bool, S::Error> {
        // Move to previous sibling, then to its last child
        // If not possible, move to parent
        let moved = if Self::find_prev_sibling(store, tree, id).await? {
            while Self::find_last_child(folded, tree, id) {}
            true
        } else {
            Self::find_parent(tree, id)
        };
        Ok(moved)
    }

    /// Move to the next message, or don't move if this is not possible.
    async fn find_next_msg(
        store: &S,
        folded: &HashSet<M::Id>,
        tree: &mut Tree<M>,
        id: &mut M::Id,
    ) -> Result<bool, S::Error> {
        if Self::find_first_child(folded, tree, id) {
            return Ok(true);
        }

        if Self::find_next_sibling(store, tree, id).await? {
            return Ok(true);
        }

        // Temporary id to avoid modifying the original one if no parent-sibling
        // can be found.
        let mut tmp_id = id.clone();
        while Self::find_parent(tree, &mut tmp_id) {
            if Self::find_next_sibling(store, tree, &mut tmp_id).await? {
                *id = tmp_id;
                return Ok(true);
            }
        }

        Ok(false)
    }

    pub async fn move_cursor_up(&mut self) -> Result<(), S::Error> {
        match &mut self.cursor {
            Cursor::Bottom | Cursor::Pseudo { parent: None, .. } => {
                if let Some(last_root_id) = self.store.last_root_id().await? {
                    let tree = self.store.tree(&last_root_id).await?;
                    let mut id = last_root_id;
                    while Self::find_last_child(&self.folded, &tree, &mut id) {}
                    self.cursor = Cursor::Msg(id);
                }
            }
            Cursor::Msg(msg) => {
                let path = self.store.path(msg).await?;
                let mut tree = self.store.tree(path.first()).await?;
                Self::find_prev_msg(&self.store, &self.folded, &mut tree, msg).await?;
            }
            Cursor::Editor { .. } => {}
            Cursor::Pseudo {
                parent: Some(parent),
                ..
            } => {
                let tree = self.store.tree(parent).await?;
                let mut id = parent.clone();
                while Self::find_last_child(&self.folded, &tree, &mut id) {}
                self.cursor = Cursor::Msg(id);
            }
        }
        self.correction = Some(Correction::MakeCursorVisible);
        Ok(())
    }

    pub async fn move_cursor_down(&mut self) -> Result<(), S::Error> {
        match &mut self.cursor {
            Cursor::Msg(msg) => {
                let path = self.store.path(msg).await?;
                let mut tree = self.store.tree(path.first()).await?;
                if !Self::find_next_msg(&self.store, &self.folded, &mut tree, msg).await? {
                    self.cursor = Cursor::Bottom;
                }
            }
            Cursor::Pseudo { parent: None, .. } => {
                self.cursor = Cursor::Bottom;
            }
            Cursor::Pseudo {
                parent: Some(parent),
                ..
            } => {
                let mut tree = self.store.tree(parent).await?;
                let mut id = parent.clone();
                while Self::find_last_child(&self.folded, &tree, &mut id) {}
                // Now we're at the previous message
                if Self::find_next_msg(&self.store, &self.folded, &mut tree, &mut id).await? {
                    self.cursor = Cursor::Msg(id);
                } else {
                    self.cursor = Cursor::Bottom;
                }
            }
            _ => {}
        }
        self.correction = Some(Correction::MakeCursorVisible);
        Ok(())
    }

    pub async fn move_cursor_up_sibling(&mut self) -> Result<(), S::Error> {
        match &mut self.cursor {
            Cursor::Bottom | Cursor::Pseudo { parent: None, .. } => {
                if let Some(last_root_id) = self.store.last_root_id().await? {
                    self.cursor = Cursor::Msg(last_root_id);
                }
            }
            Cursor::Msg(msg) => {
                let path = self.store.path(msg).await?;
                let mut tree = self.store.tree(path.first()).await?;
                Self::find_prev_sibling(&self.store, &mut tree, msg).await?;
            }
            Cursor::Editor { .. } => {}
            Cursor::Pseudo {
                parent: Some(parent),
                ..
            } => {
                let path = self.store.path(parent).await?;
                let tree = self.store.tree(path.first()).await?;
                if let Some(children) = tree.children(parent) {
                    if let Some(last_child) = children.last() {
                        self.cursor = Cursor::Msg(last_child.clone());
                    }
                }
            }
        }
        self.correction = Some(Correction::MakeCursorVisible);
        Ok(())
    }

    pub async fn move_cursor_down_sibling(&mut self) -> Result<(), S::Error> {
        match &mut self.cursor {
            Cursor::Msg(msg) => {
                let path = self.store.path(msg).await?;
                let mut tree = self.store.tree(path.first()).await?;
                if !Self::find_next_sibling(&self.store, &mut tree, msg).await?
                    && tree.parent(msg).is_none()
                {
                    self.cursor = Cursor::Bottom;
                }
            }
            Cursor::Pseudo { parent: None, .. } => {
                self.cursor = Cursor::Bottom;
            }
            _ => {}
        }
        self.correction = Some(Correction::MakeCursorVisible);
        Ok(())
    }

    pub async fn move_cursor_to_parent(&mut self) -> Result<(), S::Error> {
        match &mut self.cursor {
            Cursor::Pseudo {
                parent: Some(parent),
                ..
            } => self.cursor = Cursor::Msg(parent.clone()),
            Cursor::Msg(id) => {
                // Could also be done via retrieving the path, but it doesn't
                // really matter here
                let tree = self.store.tree(id).await?;
                Self::find_parent(&tree, id);
            }
            _ => {}
        }
        self.correction = Some(Correction::MakeCursorVisible);
        Ok(())
    }

    pub async fn move_cursor_to_root(&mut self) -> Result<(), S::Error> {
        match &mut self.cursor {
            Cursor::Pseudo {
                parent: Some(parent),
                ..
            } => {
                let path = self.store.path(parent).await?;
                self.cursor = Cursor::Msg(path.first().clone());
            }
            Cursor::Msg(msg) => {
                let path = self.store.path(msg).await?;
                *msg = path.first().clone();
            }
            _ => {}
        }
        self.correction = Some(Correction::MakeCursorVisible);
        Ok(())
    }

    pub async fn move_cursor_older(&mut self) -> Result<(), S::Error> {
        match &mut self.cursor {
            Cursor::Msg(id) => {
                if let Some(prev_id) = self.store.older_msg_id(id).await? {
                    *id = prev_id;
                }
            }
            Cursor::Bottom | Cursor::Pseudo { .. } => {
                if let Some(id) = self.store.newest_msg_id().await? {
                    self.cursor = Cursor::Msg(id);
                }
            }
            _ => {}
        }
        self.correction = Some(Correction::MakeCursorVisible);
        Ok(())
    }

    pub async fn move_cursor_newer(&mut self) -> Result<(), S::Error> {
        match &mut self.cursor {
            Cursor::Msg(id) => {
                if let Some(prev_id) = self.store.newer_msg_id(id).await? {
                    *id = prev_id;
                } else {
                    self.cursor = Cursor::Bottom;
                }
            }
            Cursor::Pseudo { .. } => {
                self.cursor = Cursor::Bottom;
            }
            _ => {}
        }
        self.correction = Some(Correction::MakeCursorVisible);
        Ok(())
    }

    pub async fn move_cursor_older_unseen(&mut self) -> Result<(), S::Error> {
        match &mut self.cursor {
            Cursor::Msg(id) => {
                if let Some(prev_id) = self.store.older_unseen_msg_id(id).await? {
                    *id = prev_id;
                }
            }
            Cursor::Bottom | Cursor::Pseudo { .. } => {
                if let Some(id) = self.store.newest_unseen_msg_id().await? {
                    self.cursor = Cursor::Msg(id);
                }
            }
            _ => {}
        }
        self.correction = Some(Correction::MakeCursorVisible);
        Ok(())
    }

    pub async fn move_cursor_newer_unseen(&mut self) -> Result<(), S::Error> {
        match &mut self.cursor {
            Cursor::Msg(id) => {
                if let Some(prev_id) = self.store.newer_unseen_msg_id(id).await? {
                    *id = prev_id;
                } else {
                    self.cursor = Cursor::Bottom;
                }
            }
            Cursor::Pseudo { .. } => {
                self.cursor = Cursor::Bottom;
            }
            _ => {}
        }
        self.correction = Some(Correction::MakeCursorVisible);
        Ok(())
    }

    pub async fn move_cursor_to_top(&mut self) -> Result<(), S::Error> {
        if let Some(first_root_id) = self.store.first_root_id().await? {
            self.cursor = Cursor::Msg(first_root_id);
            self.correction = Some(Correction::MakeCursorVisible);
        }
        Ok(())
    }

    pub async fn move_cursor_to_bottom(&mut self) {
        self.cursor = Cursor::Bottom;
        // Not really necessary; only here for consistency with other methods
        self.correction = Some(Correction::MakeCursorVisible);
    }

    pub fn scroll_up(&mut self, amount: i32) {
        self.scroll += amount;
        self.correction = Some(Correction::MoveCursorToVisibleArea);
    }

    pub fn scroll_down(&mut self, amount: i32) {
        self.scroll -= amount;
        self.correction = Some(Correction::MoveCursorToVisibleArea);
    }

    pub fn center_cursor(&mut self) {
        self.correction = Some(Correction::CenterCursor);
    }

    /// The outer `Option` shows whether a parent exists or not. The inner
    /// `Option` shows if that parent has an id.
    pub async fn parent_for_normal_reply(&self) -> Result<Option<Option<M::Id>>, S::Error> {
        Ok(match &self.cursor {
            Cursor::Bottom => Some(None),
            Cursor::Msg(id) => {
                let path = self.store.path(id).await?;
                let tree = self.store.tree(path.first()).await?;

                Some(Some(if tree.next_sibling(id).is_some() {
                    // A reply to a message that has further siblings should be a
                    // direct reply. An indirect reply might end up a lot further
                    // down in the current conversation.
                    id.clone()
                } else if let Some(parent) = tree.parent(id) {
                    // A reply to a message without younger siblings should be
                    // an indirect reply so as not to create unnecessarily deep
                    // threads. In the case that our message has children, this
                    // might get a bit confusing. I'm not sure yet how well this
                    // "smart" reply actually works in practice.
                    parent
                } else {
                    // When replying to a top-level message, it makes sense to avoid
                    // creating unnecessary new threads.
                    id.clone()
                }))
            }
            _ => None,
        })
    }

    /// The outer `Option` shows whether a parent exists or not. The inner
    /// `Option` shows if that parent has an id.
    pub async fn parent_for_alternate_reply(&self) -> Result<Option<Option<M::Id>>, S::Error> {
        Ok(match &self.cursor {
            Cursor::Bottom => Some(None),
            Cursor::Msg(id) => {
                let path = self.store.path(id).await?;
                let tree = self.store.tree(path.first()).await?;

                Some(Some(if tree.next_sibling(id).is_none() {
                    // The opposite of replying normally
                    id.clone()
                } else if let Some(parent) = tree.parent(id) {
                    // The opposite of replying normally
                    parent
                } else {
                    // The same as replying normally, still to avoid creating
                    // unnecessary new threads
                    id.clone()
                }))
            }
            _ => None,
        })
    }
}
