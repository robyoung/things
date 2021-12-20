use std::cmp;

use anyhow::Result;
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::Serialize;

use crate::id::Id;

#[derive(Serialize, JsonSchema, Clone)]
pub struct List {
    agent: u32,
    top_id: u32,
    pub items: Vec<ListItem>,
    pub name: String,
    log: OperationLog,
}

pub struct RootList {
    list: List,
    top_agent: u32,
}

impl RootList {
    pub fn new(name: impl Into<String>) -> Self {
        RootList {
            top_agent: 0,
            list: List::new(0, name),
        }
    }

    pub fn snapshot(&mut self) -> List {
        self.top_agent += 1;
        self.list.snapshot(self.top_agent)
    }

    pub fn commit(&mut self, changes: &[LogRecord]) -> Result<Vec<LogRecord>> {
        if changes[0] == *self.list.log.records.last().unwrap() {
            self.list.log.records.extend_from_slice(&changes[1..]);
            self.list.redo_all()?;
            Ok(changes.to_vec())
        } else {
            let mut new_changes = Vec::with_capacity(changes.len());
            for to_apply in changes[1..].iter() {
                let to_apply = to_apply.clone();
                if self
                    .list
                    .log
                    .changes_from(&changes[0])
                    .iter()
                    .any(|change| change.conflicts_with(&to_apply))
                {
                    return Err(ListError::CannotCommit.into());
                } else {
                    self.list.log.commit_record(to_apply.clone());
                    new_changes.push(to_apply);
                }
            }
            self.list.redo_all()?;
            Ok(new_changes)
        }
    }
}

impl List {
    fn new(agent: u32, name: impl Into<String>) -> List {
        Self {
            agent,
            top_id: 0,
            name: name.into(),
            items: vec![],
            log: Default::default(),
        }
    }

    fn snapshot(&self, agent: u32) -> Self {
        Self {
            agent,
            top_id: 0,
            name: self.name.clone(),
            items: self.items.clone(),
            log: self.log.snapshot(),
        }
    }

    pub fn add(&mut self, value: impl Into<String>) -> ListItem {
        let value = value.into();
        if let Some(item) = self.items.iter().find(|itm| value == itm.value) {
            item.clone()
        } else {
            let item = ListItem {
                id: self.next_id(),
                value,
                done: false,
            };
            self.log.push(Operation::add(&item));
            self.items.push(item.clone());
            item
        }
    }

    pub fn remove(&mut self, id: Id) {
        if let Some((i, _)) = self
            .items
            .iter()
            .enumerate()
            .find(|(_, item)| id == item.id)
        {
            let item = self.items.remove(i);
            self.log.push(Operation::remove(i, item));
        }
    }

    pub fn edit(&mut self, id: Id, value: impl Into<String>) -> Result<ListItem> {
        let item = self
            .items
            .iter_mut()
            .find(|item| item.id == id)
            .ok_or(ListError::NotFound)?
            .clone();
        let operation = Operation::edit(&item, value);

        self.log.push(operation.clone());
        self.apply(operation)?;

        Ok(item)
    }

    /// Move an item to a position in the list
    ///
    /// If the position is beyond the end of the list the item is just moved to the end.
    pub fn move_to(&mut self, id: Id, position: usize) -> Result<()> {
        let from_i = self
            .items
            .iter()
            .position(|item| item.id == id)
            .ok_or(ListError::NotFound)?;
        let operation = Operation::move_to(id, from_i, cmp::min(position, self.items.len() - 1));
        self.log.push(operation.clone());
        self.apply(operation)?;

        Ok(())
    }

    pub fn undo(&mut self) -> Result<()> {
        let operation = self.log.undo()?;
        self.revert(operation)?;
        Ok(())
    }

    /// Apply the next operation in the log
    pub fn redo(&mut self) -> Result<()> {
        let operation = self.log.redo()?;
        self.apply(operation)?;
        Ok(())
    }

    pub fn redo_all(&mut self) -> Result<()> {
        while let Ok(operation) = self.log.redo() {
            self.apply(operation)?;
        }
        Ok(())
    }

    fn apply(&mut self, operation: Operation) -> Result<()> {
        match operation {
            Operation::Add(item) => {
                self.items.push(item);
            }
            Operation::Remove(_, item) => {
                self.items.remove(
                    self.items
                        .iter()
                        .position(|itm| itm.id == item.id)
                        .ok_or(ListError::NotFound)?,
                );
            }
            Operation::Edit(old_item, new_values) => {
                let mut item = self
                    .items
                    .iter_mut()
                    .find(|item| item.id == old_item.id)
                    .ok_or(ListError::NotFound)?;
                if let Some(value) = new_values.value {
                    item.value = value;
                }
            }
            Operation::MoveTo {
                id,
                old_loc: _,
                new_loc,
            } => {
                let from_i = self
                    .items
                    .iter()
                    .position(|item| item.id == id)
                    .ok_or(ListError::NotFound)?;
                let item = self.items.remove(from_i);
                self.items.insert(new_loc, item);
            }
            Operation::Root => unreachable!(),
        }
        Ok(())
    }

    // TODO: move this out into methods on each operation
    fn revert(&mut self, operation: Operation) -> Result<()> {
        match operation {
            Operation::Add(item) => {
                self.items.remove(
                    self.items
                        .iter()
                        .position(|itm| itm.id == item.id)
                        .ok_or(ListError::NotFound)?,
                );
            }
            Operation::Remove(loc, item) => {
                self.items.insert(loc, item);
            }
            Operation::Edit(old_item, new_values) => {
                let mut item = self
                    .items
                    .iter_mut()
                    .find(|item| item.id == old_item.id)
                    .ok_or(ListError::NotFound)?;
                if new_values.value.is_some() {
                    item.value = old_item.value;
                }
            }
            Operation::MoveTo {
                id,
                old_loc,
                new_loc: _,
            } => {
                let from_i = self
                    .items
                    .iter()
                    .position(|item| item.id == id)
                    .ok_or(ListError::NotFound)?;
                let item = self.items.remove(from_i);
                self.items.insert(old_loc, item);
            }
            Operation::Root => unreachable!(),
        }
        Ok(())
    }

    pub fn iter(&self) -> impl Iterator<Item = &ListItem> {
        self.items.iter()
    }

    pub fn changes(&self) -> &[LogRecord] {
        self.log.changes()
    }

    fn next_id(&mut self) -> Id {
        self.top_id += 1;
        Id::new(self.agent, self.top_id)
    }
}

#[derive(thiserror::Error, Debug)]
enum ListError {
    #[error("Item not found")]
    NotFound,
    #[error("Cannot undo")]
    CannotUndo,
    #[error("Cannot redo")]
    CannotRedo,
    #[error("Cannot commit")]
    CannotCommit,
}

#[derive(Serialize, JsonSchema, Debug, PartialEq, Clone)]
pub struct ListItem {
    pub id: Id,
    pub value: String,
    pub done: bool,
}

/// Log of `Operation`s that have been applied to a `List`
///
/// On a `RootList` this cannot be rewound and therefore acts as
/// an append only log.
#[derive(Clone, Debug, Serialize, JsonSchema)]
struct OperationLog {
    head: usize,
    fork: usize,
    records: Vec<LogRecord>,
}

impl Default for OperationLog {
    fn default() -> Self {
        Self::new(LogRecord::root())
    }
}

impl OperationLog {
    fn new(record: LogRecord) -> Self {
        Self {
            head: 0,
            fork: 0,
            records: vec![record],
        }
    }

    fn push(&mut self, operation: Operation) {
        // TODO fail if head is not at end of log
        self.records.push(LogRecord::new(self.next_id(), operation));
        self.head += 1;
    }

    fn commit_record(&mut self, record: LogRecord) {
        self.records.push(LogRecord {
            id: self.next_id(),
            stamp: record.stamp,
            operation: record.operation,
        })
    }

    fn next_id(&self) -> u32 {
        self.records
            .last()
            .expect("log always has at least one record")
            .id
            + 1
    }

    fn undo(&mut self) -> Result<Operation> {
        if self.head <= self.fork {
            // cannot undo beyond what has been committed
            Err(ListError::CannotUndo.into())
        } else {
            let operation = self.records[self.head].operation.clone();
            self.head -= 1;
            Ok(operation)
        }
    }

    // TODO return a reference
    /// Apply the next operation
    fn redo(&mut self) -> Result<Operation> {
        if self.head == self.records.len() - 1 {
            // cannot redo beyond the end of the log
            Err(ListError::CannotRedo.into())
        } else {
            self.head += 1;
            let operation = self.records[self.head].operation.clone();
            Ok(operation)
        }
    }

    fn snapshot(&self) -> Self {
        Self::new(
            self.records
                .last()
                .expect("root log must contain at least one item")
                .clone(),
        )
    }

    fn changes(&self) -> &[LogRecord] {
        &self.records[self.fork..self.head + 1]
    }

    fn changes_from(&self, record: &LogRecord) -> &[LogRecord] {
        if let Some(i) = self.records.iter().position(|r| r == record) {
            &self.records[i..]
        } else {
            &self.records[self.records.len()..]
        }
    }
}

#[derive(Clone, Debug, Serialize, JsonSchema, PartialEq)]
pub struct LogRecord {
    id: u32,
    stamp: DateTime<Utc>,
    operation: Operation,
}

impl LogRecord {
    fn new(id: u32, operation: Operation) -> Self {
        Self {
            id,
            stamp: Utc::now(),
            operation,
        }
    }

    fn root() -> Self {
        Self::new(0, Operation::Root)
    }

    fn conflicts_with(&self, other: &LogRecord) -> bool {
        match (&self.operation, &other.operation) {
            (Operation::Add(a), Operation::Add(b)) => a.id == b.id || a.value == b.value,
            (Operation::Root, _) => false,
            _ => {
                dbg!(&self.operation, &other.operation);
                todo!();
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, JsonSchema, PartialEq)]
enum Operation {
    Root,
    Add(ListItem),
    Remove(usize, ListItem),
    Edit(ListItem, ListItemUpdate),
    MoveTo {
        id: Id,
        old_loc: usize,
        new_loc: usize,
    },
}

#[derive(Clone, Debug, Serialize, JsonSchema, PartialEq)]
struct ListItemUpdate {
    value: Option<String>,
    done: Option<bool>,
}

impl Operation {
    pub fn add(item: &ListItem) -> Self {
        Operation::Add(item.clone())
    }

    pub fn remove(loc: usize, item: ListItem) -> Self {
        Operation::Remove(loc, item)
    }

    pub fn edit(item: &ListItem, new_value: impl Into<String>) -> Self {
        Operation::Edit(
            item.clone(),
            ListItemUpdate {
                value: Some(new_value.into()),
                done: None,
            },
        )
    }

    pub fn move_to(id: Id, from_loc: usize, to_loc: usize) -> Self {
        Self::MoveTo {
            id,
            old_loc: from_loc,
            new_loc: to_loc,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_delete_add_gives_new_item() {
        let mut list = RootList::new("shopping").snapshot();
        let potatoes1 = list.add("potatoes");
        list.remove(potatoes1.id);
        let potatoes2 = list.add("potatoes");
        assert_ne!(potatoes1, potatoes2);
    }

    #[test]
    fn add_add_gives_same_item() {
        let mut list = RootList::new("shopping").snapshot();
        let potatoes1 = list.add("potatoes");
        let potatoes2 = list.add("potatoes");
        assert_eq!(potatoes1, potatoes2);
    }

    #[test]
    fn edit_an_item() {
        let mut list = RootList::new("shopping").snapshot();
        let potatoes = list.add("potatoes");
        list.edit(potatoes.id, "apples").unwrap();
        assert_eq!(list_values(&list), vec!["apples"]);
    }

    #[test]
    fn move_an_item() {
        let mut list = RootList::new("shopping").snapshot();
        let potatoes = list.add("potatoes");
        list.add("tomatoes");
        list.add("crisps");

        list.move_to(potatoes.id, 0).unwrap();
        assert_eq!(list_values(&list), vec!["potatoes", "tomatoes", "crisps"]);
        list.move_to(potatoes.id, 2).unwrap();
        assert_eq!(list_values(&list), vec!["tomatoes", "crisps", "potatoes"]);
        list.move_to(potatoes.id, 5).unwrap();
        assert_eq!(list_values(&list), vec!["tomatoes", "crisps", "potatoes"]);
    }

    mod undo {
        use super::*;

        #[test]
        fn undo_add() {
            let mut list = RootList::new("shopping").snapshot();
            list.add("potatoes");
            list.add("tomatoes");
            assert_eq!(list_values(&list), vec!["potatoes", "tomatoes"]);
            list.undo().unwrap();
            assert_eq!(list_values(&list), vec!["potatoes"]);
        }

        #[test]
        fn undo_beyond_start_fails() {
            let mut list = RootList::new("shopping").snapshot();
            list.add("potatoes");
            list.undo().unwrap();
            let err = list.undo().err().unwrap();
            assert!(matches!(
                err.downcast_ref::<ListError>(),
                Some(&ListError::CannotUndo),
            ));
        }

        #[test]
        fn redo_add() {
            let mut list = RootList::new("shopping").snapshot();
            list.add("potatoes");
            list.add("tomatoes");
            list.undo().unwrap();
            list.redo().unwrap();
            assert_eq!(list_values(&list), vec!["potatoes", "tomatoes"]);
        }

        #[test]
        fn redo_beyond_end_fails() {
            let mut list = RootList::new("shopping").snapshot();
            list.add("potatoes");
            list.add("tomatoes");
            list.undo().unwrap();
            list.redo().unwrap();
            let err = list.redo().err().unwrap();
            assert!(matches!(
                err.downcast_ref::<ListError>(),
                Some(&ListError::CannotRedo)
            ));
            assert_eq!(list_values(&list), vec!["potatoes", "tomatoes"]);
        }

        #[test]
        fn undo_remove() {
            let mut list = RootList::new("shopping").snapshot();
            let potatoes = list.add("potatoes");
            list.add("tomatoes");
            list.remove(potatoes.id);
            assert_eq!(list_values(&list), vec!["tomatoes"]);
            list.undo().unwrap();
            assert_eq!(list_values(&list), vec!["potatoes", "tomatoes"]);
        }

        #[test]
        fn redo_remove() {
            let mut list = RootList::new("shopping").snapshot();
            let potatoes = list.add("potatoes");
            list.add("tomatoes");
            list.remove(potatoes.id);
            list.undo().unwrap();
            list.redo().unwrap();
            assert_eq!(list_values(&list), vec!["tomatoes"]);
        }

        #[test]
        fn undo_edit() {
            let mut list = RootList::new("shopping").snapshot();
            let potatoes = list.add("potatoes");
            list.add("tomatoes");
            list.edit(potatoes.id, "spuds").unwrap();
            assert_eq!(list_values(&list), vec!["spuds", "tomatoes"]);
            list.undo().unwrap();
            assert_eq!(list_values(&list), vec!["potatoes", "tomatoes"]);
        }

        #[test]
        fn redo_edit() {
            let mut list = RootList::new("shopping").snapshot();
            let potatoes = list.add("potatoes");
            list.add("tomatoes");
            list.edit(potatoes.id, "spuds").unwrap();
            list.undo().unwrap();
            list.redo().unwrap();
            assert_eq!(list_values(&list), vec!["spuds", "tomatoes"]);
        }

        #[test]
        fn undo_move_to() {
            let mut list = RootList::new("shopping").snapshot();
            let potatoes = list.add("potatoes");
            list.add("tomatoes");
            list.move_to(potatoes.id, 1).unwrap();
            assert_eq!(list_values(&list), vec!["tomatoes", "potatoes"]);
            list.undo().unwrap();
            assert_eq!(list_values(&list), vec!["potatoes", "tomatoes"]);
        }

        #[test]
        fn redo_move_to() {
            let mut list = RootList::new("shopping").snapshot();
            let potatoes = list.add("potatoes");
            list.add("tomatoes");
            list.move_to(potatoes.id, 1).unwrap();
            assert_eq!(list_values(&list), vec!["tomatoes", "potatoes"]);
            list.undo().unwrap();
            assert_eq!(list_values(&list), vec!["potatoes", "tomatoes"]);
            list.redo().unwrap();
            assert_eq!(list_values(&list), vec!["tomatoes", "potatoes"]);
        }
    }

    mod commit {
        use super::*;

        #[test]
        fn commit_with_no_intervening_records() {
            let mut root = RootList::new("shopping");
            let mut list = root.snapshot();
            list.add("potatoes");
            root.commit(list.changes()).unwrap();

            let list = root.snapshot();
            assert_eq!(list_values(&list), vec!["potatoes"]);
        }

        #[test]
        fn commit_adds() {
            let mut root = RootList::new("shopping");
            let mut list1 = root.snapshot();
            let mut list2 = root.snapshot();
            list1.add("potatoes");
            list2.add("tomatoes");
            root.commit(list1.changes()).unwrap();
            root.commit(list2.changes()).unwrap();

            let list = root.snapshot();
            assert_eq!(list_values(&list), vec!["potatoes", "tomatoes"]);
        }
        /*
        #[test]
        fn two_non_conflicting_adds() {
            let mut list1 = List::new("shopping");
            list1.add("potatoes");
            let mut list2 = list1.clone();
            list1.add("apples");
            list2.add("crisps");

            list1.merge(list2);

            assert_eq!(list_values(&list1), vec!["potatoes", "apples", "crisps"]);
        }

        #[test]
        fn two_conflicting_adds() {
            let mut list1 = List::new("shopping");
            list1.add("potatoes");
            let mut list2 = list1.clone();
            list1.add("apples");
            list2.add("apples");

            list1.merge(list2);

            assert_eq!(list_values(&list1), vec!["potatoes", "apples"]);
        }
        */
    }

    fn list_values(list: &List) -> Vec<&str> {
        list.iter().map(|item| item.value.as_str()).collect()
    }
}
