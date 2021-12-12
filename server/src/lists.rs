use std::{cmp, collections::HashSet};

use anyhow::Result;
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::Serialize;

// TODO: maybe make it so only snapshots are serializable
#[derive(Serialize, JsonSchema, Clone)]
pub struct List {
    top_id: u32,
    pub items: Vec<ListItem>,
    pub name: String,
    log: Log,
}

pub struct RootList(List);

impl RootList {
    pub fn new(name: impl Into<String>) -> Self {
        RootList(List {
            top_id: 0,
            name: name.into(),
            items: vec![],
            log: Default::default(),
        })
    }

    pub fn snapshot(&self) -> List {
        List {
            top_id: self.0.top_id,
            name: self.0.name.clone(),
            items: self.0.items.clone(),
            log: self.0.log.snapshot(),
        }
    }
}

impl List {
    pub fn add(&mut self, value: impl Into<String>) -> ListItem {
        let value = value.into();
        if let Some(item) = self.items.iter().find(|itm| value == itm.value) {
            item.clone()
        } else {
            let item = ListItem {
                id: self.next_id(),
                value,
            };
            self.log.push(Operation::add(&item));
            self.items.push(item.clone());
            item
        }
    }

    pub fn remove(&mut self, id: u32) {
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

    pub fn edit(&mut self, id: u32, value: impl Into<String>) -> Result<ListItem> {
        let mut item = self
            .items
            .iter_mut()
            .find(|item| item.id == id)
            .ok_or(ListError::NotFound)?;
        let new_value: String = value.into();
        self.log.push(Operation::edit(&item, &new_value));
        item.value = new_value;
        Ok(item.clone())
    }

    /// Move an item to a position in the list
    ///
    /// If the position is beyond the end of the list the item is just moved to the end.
    pub fn move_to(&mut self, id: u32, position: usize) -> Result<()> {
        let from_i = self
            .items
            .iter()
            .position(|item| item.id == id)
            .ok_or(ListError::NotFound)?;

        let item = self.items.remove(from_i);
        self.items
            .insert(cmp::min(position, self.items.len()), item);

        Ok(())
    }

    pub fn undo(&mut self) -> Result<()> {
        let operation = self.log.undo()?;
        self.apply_undo(operation)?;
        Ok(())
    }

    fn apply_undo(&mut self, operation: Operation) -> Result<()> {
        match operation {
            Operation::Add(item) => {
                self.items.remove(
                    self.items
                        .iter()
                        .position(|itm| itm.id == item.id)
                        .ok_or(ListError::NotFound)?,
                );
            }
            Operation::Remove { loc, id, value } => {
                self.items.insert(loc, ListItem { id, value });
            }
            Operation::Edit {
                id,
                old_value,
                new_value: _,
            } => {
                let mut item = self
                    .items
                    .iter_mut()
                    .find(|item| item.id == id)
                    .ok_or(ListError::NotFound)?;
                item.value = old_value;
            }
            Operation::Root => unreachable!(),
        }
        Ok(())
    }

    pub fn iter(&self) -> impl Iterator<Item = &ListItem> {
        self.items.iter()
    }

    pub fn merge(&mut self, mut other: List) {
        // TODO: fix this it is bad
        let values = self
            .items
            .iter()
            .map(|item| &item.value)
            .collect::<HashSet<&String>>();
        let mut to_add = vec![];
        for (i, item) in other.iter().enumerate() {
            if !values.contains(&item.value) {
                to_add.push(i);
            }
        }
        for i in to_add {
            self.items.push(other.items.swap_remove(i));
        }
    }

    fn next_id(&mut self) -> u32 {
        self.top_id += 1;
        self.top_id
    }
}

#[derive(thiserror::Error, Debug)]
enum ListError {
    #[error("Item not found")]
    NotFound,
    #[error("Cannot undo")]
    CannotUndo,
}

#[derive(Serialize, JsonSchema, Debug, PartialEq, Clone)]
pub struct ListItem {
    pub id: u32,
    pub value: String,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
struct Log {
    head: usize,
    fork: usize,
    records: Vec<LogRecord>,
}

impl Default for Log {
    fn default() -> Self {
        Self::new(LogRecord::root())
    }
}

impl Log {
    fn new(record: LogRecord) -> Self {
        Self {
            head: 0,
            fork: 0,
            records: vec![record],
        }
    }
    fn push(&mut self, operation: Operation) {
        self.records.push(LogRecord::new(
            self.records
                .last()
                .expect("log always has at least one record")
                .id
                + 1,
            operation,
        ));
        self.head += 1;
    }

    fn undo(&mut self) -> Result<Operation> {
        if self.head == 0 {
            Err(ListError::CannotUndo.into())
        } else {
            let operation = self.records[self.head].operation.clone();
            self.head -= 1;
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
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
struct LogRecord {
    id: u32,
    stamp: DateTime<Utc>,
    operation: Operation,
    // TODO add operation
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
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
enum Operation {
    Root,
    Add(ListItem),
    Remove {
        loc: usize,
        id: u32,
        value: String,
    },
    Edit {
        id: u32,
        old_value: String,
        new_value: String,
    },
}

impl Operation {
    pub fn add(item: &ListItem) -> Self {
        Operation::Add(item.clone())
    }

    pub fn remove(loc: usize, item: ListItem) -> Self {
        Operation::Remove {
            loc,
            id: item.id,
            value: item.value,
        }
    }

    pub fn edit(item: &ListItem, new_value: &str) -> Self {
        Operation::Edit {
            id: item.id,
            old_value: item.value.clone(),
            new_value: new_value.to_string(),
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
        fn undo_edit() {
            let mut list = RootList::new("shopping").snapshot();
            let potatoes = list.add("potatoes");
            list.add("tomatoes");
            list.edit(potatoes.id, "spuds").unwrap();
            assert_eq!(list_values(&list), vec!["spuds", "tomatoes"]);
            list.undo().unwrap();
            assert_eq!(list_values(&list), vec!["potatoes", "tomatoes"]);
        }
    }

    mod merge {
        use super::*;

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
