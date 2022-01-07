//! Wire protocol
//! ```
//! {
//!   "fork": u32,  // id of change in list
//!   "changes": [
//!     {
//!       "timestamp": 12345,
//!       "operation": "add",
//!       "item": {"id": "1/1", "title": "apples", "done": false}},
//!     },
//!
//!     {
//!       "timestamp": 12346,
//!       "operation":
//!     }
//!
//!
//!
//!   ]
//! }
//! ```
use std::collections::HashMap;

use anyhow::Result;
use chrono::{DateTime, Utc};

use crate::id::Id;

#[derive(Debug)]
pub struct ServerList {
    list: List,
    max_agent_id: u32,
}

impl ServerList {
    pub fn new() -> Self {
        Self {
            max_agent_id: 0,
            list: List::new(0),
        }
    }

    pub fn snapshot(&mut self) -> List {
        self.max_agent_id += 1;
        self.list.snapshot(self.max_agent_id)
    }

    pub fn commit(&mut self, changes: &[Change]) -> Result<Vec<Change>> {
        if !self.list.changes.is_at_head() {
            unreachable!("root list must always be at head");
        }
        // TODO: handle the same set of changes being committed again
        let changes = squash_changes(changes);
        if changes[0] == *self.list.changes.changes.last().unwrap() {
            // TODO: double check if this branch is required. I think `transform` will just do the
            // right thing
            return Ok(self
                .list
                .apply_all(&changes[1..])
                .map(|_| changes[1..].to_vec())?);
        } else {
            let mut confirmed_changes = self.list.changes.changes_since(&changes[0]).to_vec();
            let incoming_changes = &changes[1..];
            let new_changes = transform(&confirmed_changes, incoming_changes);

            return Ok(self.list.apply_all(&new_changes).map(|_| {
                confirmed_changes.extend_from_slice(&new_changes);
                confirmed_changes
            })?);
        }
    }
}

#[derive(Debug)]
pub struct List {
    agent_id: u32,
    max_item_id: u32,
    items: Vec<ListItem>,
    changes: ChangeLog,
}

impl List {
    fn new(agent_id: u32) -> Self {
        Self {
            agent_id,
            max_item_id: 0,
            items: vec![],
            changes: ChangeLog::new(),
        }
    }

    fn snapshot(&self, agent_id: u32) -> Self {
        Self {
            agent_id,
            max_item_id: 0,
            items: self.items.clone(),
            changes: self.changes.clone(), // TODO: do we need the whole change log?
        }
    }

    fn iter(&self) -> impl Iterator<Item = &ListItem> {
        self.items.iter()
    }

    fn next_id(&mut self) -> Id {
        self.max_item_id += 1;
        Id::new(self.agent_id, self.max_item_id)
    }

    fn push(&mut self, op: Operation) -> Result<()> {
        // TODO: figure out how to avoid the clone
        let change = self.changes.push(op).clone();
        self.apply(&change).map_err(|err| {
            self.changes.pop();
            err
        })
    }

    fn apply_next(&mut self) -> Result<()> {
        todo!();
    }

    fn apply_commit(&mut self, changes: &[Change]) -> Result<()> {
        // roll back changes to commit
        for _ in 0..self.changes_to_commit().len() - 1 {
            self.revert_current().expect("revert cannot fail");
        }

        // apply the new changes
        let result = self.apply_all(changes);

        // update the fork point
        self.changes.fork = self.changes.head;

        result
    }

    /// Apply all changes or apply none if any fail
    fn apply_all(&mut self, changes: &[Change]) -> Result<()> {
        // TODO: test failure and rollback
        for (i, change) in changes.iter().enumerate() {
            self.changes.push_change(change);
            if let Err(err) = self.apply(change) {
                for _ in 0..i {
                    self.revert_current().expect("undo an apply must not fail");
                }
                return Err(err);
            }
        }
        Ok(())
    }

    fn apply(&mut self, change: &Change) -> Result<()> {
        use Operation::*;
        match &change.operation {
            Add(item) => {
                self.items.push(item.clone());
            }
            Remove(item) => {
                self.items.remove(
                    self.items
                        .iter()
                        .position(|itm| itm.id == item.id)
                        .ok_or(ListError::NotFound)?,
                );
            }
            Edit(old_item, new_item) => {
                let item = self
                    .items
                    .iter_mut()
                    .find(|itm| itm.id == old_item.id)
                    .ok_or(ListError::NotFound)?;
                *item = new_item.clone();
            }
            Root => unreachable!("cannot apply the root operation"),
        }
        Ok(())
    }

    fn revert(&mut self, change: &Change) -> Result<()> {
        use Operation::*;
        match &change.operation {
            Add(item) => {
                self.items.remove(
                    self.items
                        .iter()
                        .position(|itm| itm.id == item.id)
                        .ok_or(ListError::NotFound)?,
                );
            }
            Remove(item) => {
                self.items.push(item.clone());
                // TODO: sort?
            }
            Edit(old_item, new_item) => {
                let item = self
                    .items
                    .iter_mut()
                    .find(|itm| itm.id == new_item.id)
                    .ok_or(ListError::NotFound)?;
                *item = old_item.clone();
            }
            Root => unreachable!("cannot revert the root operation"),
        }

        Ok(())
    }

    fn revert_current(&mut self) -> Result<()> {
        let change = self
            .changes
            .previous()
            .ok_or_else(|| ListError::NotFound)?
            .clone();
        self.revert(&change)
    }

    pub fn add(&mut self, title: impl Into<String>) -> ListItem {
        let title = title.into();
        if let Some(item) = self.items.iter().find(|item| title == item.title) {
            item.clone()
        } else {
            let item = ListItem {
                id: self.next_id(),
                title,
                done: false,
                order: self
                    .items
                    .iter()
                    .map(|item| item.order)
                    .fold(0f32, f32::max)
                    + 1f32,
            };
            self.push(Operation::Add(item.clone()))
                .expect("add cannot fail");
            item
        }
    }

    pub fn remove(&mut self, id: impl Into<Id>) -> Result<ListItem> {
        let id = id.into();
        let item = self
            .items
            .iter()
            .find(|itm| itm.id == id)
            .ok_or(ListError::NotFound)?
            .clone();
        self.push(Operation::Remove(item.clone()))?;
        Ok(item)
    }

    pub fn update(&mut self, id: impl Into<Id>, update: UpdateListItem) -> Result<ListItem> {
        let id = id.into();
        let old_item = self
            .items
            .iter()
            .find(|item| item.id == id)
            .ok_or(ListError::NotFound)?
            .clone();
        let new_item = update.update(&old_item);
        self.push(Operation::Edit(old_item, new_item.clone()))?;
        Ok(new_item)
    }

    pub fn place_after(&mut self, move_id: impl Into<Id>, after_id: Option<Id>) -> Result<()> {
        let move_id = move_id.into();

        let move_item_position = self
            .items
            .iter()
            .position(|item| item.id == move_id)
            .ok_or(ListError::NotFound)?;

        let (low, high) = if let Some(after_id) = after_id {
            let after_id = after_id.into();
            let position = self
                .items
                .iter()
                .position(|itm| itm.id == after_id)
                .ok_or(ListError::NotFound)?;
            if position == self.items.len() {
                (
                    self.items[position].order,
                    self.items[position].order.floor() + 1f32,
                )
            } else {
                (self.items[position].order, self.items[position + 1].order)
            }
        } else {
            if self.items[0].id == self.items[move_item_position].id {
                return Ok(());
            }
            (0f32, self.items[0].order)
        };

        self.items[move_item_position].order = (low + high) / 2f32;

        self.items
            .sort_by(|a, b| a.order.partial_cmp(&b.order).unwrap());

        Ok(())
    }

    pub fn changes_to_commit(&self) -> &[Change] {
        self.changes.to_commit()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ListItem {
    pub id: Id,
    pub title: String,
    pub done: bool,
    pub order: f32,
}

impl Into<Id> for ListItem {
    fn into(self) -> Id {
        self.id
    }
}

#[derive(Default)]
pub struct UpdateListItem {
    title: Option<String>,
    done: Option<bool>,
    order: Option<f32>,
}

impl UpdateListItem {
    pub fn new() -> Self {
        Default::default()
    }
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn tick(self) -> Self {
        self.done(true)
    }

    pub fn untick(self) -> Self {
        self.done(false)
    }

    pub fn done(mut self, done: bool) -> Self {
        self.done = Some(done);
        self
    }

    pub fn order(mut self, order: f32) -> Self {
        self.order = Some(order);
        self
    }

    fn update(self, old_item: &ListItem) -> ListItem {
        let mut new_item = old_item.clone();
        if let Some(title) = self.title {
            new_item.title = title;
        }
        if let Some(done) = self.done {
            new_item.done = done;
        }
        if let Some(order) = self.order {
            new_item.order = order;
        }

        new_item
    }
}

#[derive(thiserror::Error, Debug)]
enum ListError {
    #[error("Item not found")]
    NotFound,
}

#[derive(Clone, PartialEq, Debug)]
pub enum Operation {
    Root,
    Add(ListItem),
    Remove(ListItem),
    Edit(ListItem, ListItem),
}

enum TransformResult {
    Apply(Change),
    Skip(Change),
}

#[derive(Clone, PartialEq, Debug)]
pub struct Change {
    timestamp: DateTime<Utc>,
    operation: Operation,
}

impl Change {
    fn root() -> Self {
        Self::new(Operation::Root)
    }

    fn new(operation: Operation) -> Self {
        Self {
            timestamp: Utc::now(),
            operation,
        }
    }

    fn update_item_id(&mut self, id_map: &HashMap<Id, Id>) {
        match &mut self.operation {
            Operation::Remove(item) if id_map.contains_key(&item.id) => item.id = id_map[&item.id],
            Operation::Edit(from_item, to_item) if id_map.contains_key(&from_item.id) => {
                let new_id = id_map[&from_item.id];
                from_item.id = new_id;
                to_item.id = new_id;
            }
            _ => {}
        }
    }
}

#[derive(Clone, Debug)]
pub struct ChangeLog {
    head: usize,
    fork: usize,
    changes: Vec<Change>,
}

impl ChangeLog {
    fn new() -> Self {
        Self {
            head: 0,
            fork: 0,
            changes: vec![Change::root()],
        }
    }

    fn push(&mut self, op: Operation) -> &Change {
        if !self.is_at_head() {
            panic!("cannot push when not at head")
        }
        let change = Change::new(op);
        self.changes.push(change);
        self.next().expect("just pushed one")
    }

    fn push_change(&mut self, change: &Change) -> &Change {
        self.changes.push(change.clone());
        self.next().expect("just pushed one")
    }

    fn changes_since(&self, change: &Change) -> &[Change] {
        if let Some(i) = self.changes.iter().position(|c| c == change) {
            &self.changes[i + 1..]
        } else {
            unreachable!("the change must exist!")
        }
    }

    fn is_at_head(&self) -> bool {
        // TODO: this means the fork change is included in changes_to_commit
        self.head == self.changes.len() - 1
    }

    fn pop(&mut self) -> Option<Change> {
        if !self.is_at_head() {
            panic!("cannot pop if not at head")
        }
        self.head -= 1;
        self.changes.pop()
    }

    fn next(&mut self) -> Option<&Change> {
        if self.head < self.changes.len() - 1 {
            self.head += 1;
            Some(&self.changes[self.head])
        } else {
            None
        }
    }

    fn previous(&mut self) -> Option<&Change> {
        if self.head <= self.fork {
            // cannot undo beyond what has been committed
            None
        } else {
            let change = &self.changes[self.head];
            self.head -= 1;
            Some(&change)
        }
    }

    fn to_commit(&self) -> &[Change] {
        &self.changes[self.fork..self.head + 1]
    }
}

fn squash_changes(changes: &[Change]) -> Vec<Change> {
    let mut out_changes = vec![];

    for change in changes {
        if !out_changes
            .iter_mut()
            .rev()
            .any(|out_change| squash_one(out_change, change))
        {
            out_changes.push(change.clone());
        }
    }

    out_changes
}

fn squash_one(out_change: &mut Change, change: &Change) -> bool {
    match (&mut out_change.operation, &change.operation) {
        (Operation::Add(out_item), Operation::Edit(old_item, new_item)) if out_item == old_item => {
            *out_item = new_item.clone();
            true
        }
        (_, _) => false,
    }
}

fn transform(confirmed_changes: &[Change], incoming_changes: &[Change]) -> Vec<Change> {
    let mut new_changes = Vec::with_capacity(incoming_changes.len());
    let mut id_map = HashMap::new();

    for i in 0..incoming_changes.len() {
        let mut incoming_change = incoming_changes[i].clone();
        incoming_change.update_item_id(&id_map);

        match transform_one(confirmed_changes, &incoming_change) {
            TransformResult::Apply(change) => new_changes.push(change),
            TransformResult::Skip(conflicting_change) => {
                if let (
                    Operation::Add(incoming_item),
                    Operation::Add(existing_item) | Operation::Edit(existing_item, _),
                ) = (incoming_change.operation, conflicting_change.operation)
                {
                    id_map.insert(incoming_item.id, existing_item.id);
                }
            }
        }
    }

    new_changes
}

fn transform_one(confirmed_changes: &[Change], incoming_change: &Change) -> TransformResult {
    use Operation::*;
    for confirmed_change in confirmed_changes.iter().rev() {
        match (&confirmed_change.operation, &incoming_change.operation) {
            (Add(confirmed_item), Add(incoming_item)) => {
                if confirmed_item.id == incoming_item.id {
                    // replayed changes should be filtered by id earlier
                    unreachable!("two adds cannot have the same id")
                } else if confirmed_item.title == incoming_item.title {
                    return TransformResult::Skip(confirmed_change.clone());
                }
            }
            (Add(confirmed_item), Edit(incoming_item, _)) => {
                if confirmed_item.id == incoming_item.id {
                    // can happen if ids have been mapped from a skipped add
                    if confirmed_item.title != incoming_item.title {
                        // TODO: create a new item item for the edit
                        //  - how to calculate new ID? Use UUIDs?
                        //  - how to address duplicate titles? let it bubble up to the user?
                        todo!("this is hard to decide")
                    } else {
                        return TransformResult::Apply(incoming_change.clone());
                    }
                }
            }
            (Edit(confirmed_item, confirmed_new_item), Add(incoming_item)) => {
                if confirmed_item.id == incoming_item.id {
                    // replaying changes should be filtered by id earlier
                    unreachable!("cannot add what is already edited")
                } else if incoming_item.title == confirmed_new_item.title {
                    return TransformResult::Skip(confirmed_change.clone());
                }
            }
            (Edit(confirmed_item, confirmed_new_item), Edit(incoming_item, incoming_new_item)) => {
                if confirmed_item.id == incoming_item.id {
                    if confirmed_item.title == incoming_item.title {
                        if confirmed_new_item.title == incoming_new_item.title {
                            // last write wins
                            return TransformResult::Apply(incoming_change.clone());
                        } else {
                            // TODO: should we create a new item? how to handle ids?
                            todo!("this is hard to decide")
                        }
                    } else {
                        todo!("multiple edits, hard to decide")
                    }
                }
            }
            (Remove(confirmed_item), Remove(incoming_item)) => {
                if confirmed_item.id == incoming_item.id {
                    return TransformResult::Skip(confirmed_change.clone());
                }
            }
            (Remove(confirmed_item), Edit(incoming_item, _)) => {
                if confirmed_item.id == incoming_item.id {
                    // TODO: should we add a new item? how to handle ids?
                    todo!("this is hard to decide")
                }
            }
            (Remove(_), Add(_)) => {}
            (Add(confirmed_item), Remove(incoming_item)) => {
                if confirmed_item.id == incoming_item.id {
                    // TODO: is this right?
                    return TransformResult::Apply(incoming_change.clone());
                }
            }
            (Edit(_, _), Remove(_)) => {}
            (Root, _) => {}
            (_, Root) => unreachable!("root is always confirmed"),
        }
    }
    TransformResult::Apply(incoming_change.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[ctor::ctor]
    fn init_color_backtrace() {
        color_backtrace::install();
    }

    mod operations {
        use super::*;

        #[test]
        fn add_remove_add_gives_new_item() {
            let mut list = ServerList::new().snapshot();
            let potatoes1 = list.add("potatoes");
            list.remove(potatoes1.id).unwrap();
            let potatoes2 = list.add("potatoes");
            assert_ne!(potatoes1, potatoes2);
        }

        #[test]
        fn remove_by_item() {
            let mut list = ServerList::new().snapshot();
            let potatoes = list.add("potatoes");
            list.add("carrots");
            list.remove(potatoes).unwrap();
            assert_eq!(list_titles(&list), ["carrots"]);
        }

        #[test]
        fn add_add_gives_same_item() {
            let mut list = ServerList::new().snapshot();
            let potatoes1 = list.add("potatoes");
            let potatoes2 = list.add("potatoes");
            assert_eq!(potatoes1, potatoes2);
        }

        #[test]
        fn edit_an_item() {
            let mut list = ServerList::new().snapshot();
            let potatoes = list.add("potatoes");
            list.update(potatoes, UpdateListItem::new().title("potato"))
                .unwrap();
            assert_eq!(list_titles(&list), ["potato"]);
        }

        #[test]
        fn move_an_item() {
            let mut list = ServerList::new().snapshot();
            let potatoes = list.add("potatoes");
            let tomatoes = list.add("tomatoes");
            list.add("crisps");

            assert_eq!(list_titles(&list), ["potatoes", "tomatoes", "crisps"]);
            list.place_after(potatoes.id, Some(tomatoes.id)).unwrap();
            assert_eq!(list_titles(&list), ["tomatoes", "potatoes", "crisps"]);
        }

        #[test]
        fn move_item_to_start() {
            let mut list = ServerList::new().snapshot();
            let potatoes = list.add("potatoes");
            let tomatoes = list.add("tomatoes");
            list.add("crisps");

            assert_eq!(list_titles(&list), ["potatoes", "tomatoes", "crisps"]);
            list.place_after(tomatoes.id, None).unwrap();
            assert_eq!(list_titles(&list), ["tomatoes", "potatoes", "crisps"]);
        }
    }

    mod commit {
        use super::*;

        #[test]
        fn commit_with_no_intervening_changes() {
            let mut server = ServerList::new();
            let mut list = server.snapshot();
            list.add("potatoes");
            let changes_in = list.changes_to_commit();
            let changes_out = server.commit(changes_in).unwrap();
            assert_eq!(changes_out.len(), 1);

            let list = server.snapshot();
            assert_eq!(list_titles(&list), vec!["potatoes"]);
        }

        #[test]
        fn commit_adds() {
            let mut server = ServerList::new();
            let mut list1 = server.snapshot();
            let mut list2 = server.snapshot();
            list1.add("potatoes");
            list2.add("tomatoes");
            let changes_out1 = server.commit(list1.changes_to_commit()).unwrap();
            assert_eq!(changes_out1.len(), 1);
            let changes_out2 = server.commit(list2.changes_to_commit()).unwrap();
            assert_eq!(changes_out2.len(), 2);

            let list = server.snapshot();
            assert_eq!(list_titles(&list), vec!["potatoes", "tomatoes"]);
        }

        #[test]
        fn commit_changes_back_to_list() {
            let mut server = ServerList::new();
            let mut list1 = server.snapshot();
            list1.add("potatoes");
            let changes_in = list1.changes_to_commit().to_vec();
            assert_eq!(changes_in.len(), 2);
            let changes_out = server.commit(&changes_in).unwrap();
            list1.apply_commit(&changes_out).unwrap();

            assert_eq!(list1.changes_to_commit().len(), 1);
        }

        #[test]
        fn two_conflicting_adds() {
            let mut server = ServerList::new();
            let mut list1 = server.snapshot();
            let mut list2 = server.snapshot();
            list1.add("apples");
            list2.add("apples");

            let changes1 = server.commit(list1.changes_to_commit()).unwrap();
            let changes2 = server.commit(list2.changes_to_commit()).unwrap();

            assert_eq!(changes1, changes2);
        }

        #[test]
        fn conflicting_add_then_edit() {
            let mut server = ServerList::new();
            let mut list1 = server.snapshot();
            let mut list2 = server.snapshot();
            list1.add("apples");
            let item = list2.add("apples");
            list2
                .update(item, UpdateListItem::new().title("beans"))
                .unwrap();

            let changes1 = server.commit(list1.changes_to_commit()).unwrap();
            let changes2 = server.commit(list2.changes_to_commit()).unwrap();

            assert_eq!(changes1.len(), 1);
            assert_eq!(changes2.len(), 2);

            assert_eq!(list_titles(&server.list), vec!["apples", "beans"]);
        }

        #[test]
        fn conflicting_add_on_edit() {
            let mut server = ServerList::new();
            let mut list1 = server.snapshot();
            let mut list2 = server.snapshot();
            let item = list1.add("apples");
            list1
                .update(item, UpdateListItem::new().title("beans"))
                .unwrap();
            list2.add("beans");

            let changes1 = server.commit(list1.changes_to_commit()).unwrap();
            let changes2 = server.commit(list2.changes_to_commit()).unwrap();

            assert_eq!(changes1, changes2);
        }

        #[test]
        fn conflicting_add_then_edits_on_both() {
            let mut server = ServerList::new();
            let mut list1 = server.snapshot();
            let mut list2 = server.snapshot();

            let item = list1.add("apples");
            list1
                .update(item, UpdateListItem::new().title("beans"))
                .unwrap();

            let item = list2.add("beans");
            list2
                .update(item, UpdateListItem::new().title("apples"))
                .unwrap();

            let changes1 = server.commit(list1.changes_to_commit()).unwrap();
            let changes2 = server.commit(list2.changes_to_commit()).unwrap();

            assert_eq!(changes1.len(), 1);
            assert_eq!(changes2.len(), 2);

            assert_eq!(list_titles(&server.list), vec!["beans", "apples"]);
        }
    }

    fn list_titles(list: &List) -> Vec<&str> {
        list.iter().map(|item| item.title.as_str()).collect()
    }
}
