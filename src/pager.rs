use crate::error::{DbError, Result};

pub const PAGE_SIZE: usize = 4096;
const SLOT_OVERHEAD: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PageId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SlotId(pub usize);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlottedPage {
    pub page_id: PageId,
    slots: Vec<Option<Vec<u8>>>,
}

impl SlottedPage {
    pub fn new(page_id: PageId) -> Self {
        Self {
            page_id,
            slots: Vec::new(),
        }
    }

    pub fn insert(&mut self, record: &[u8]) -> Result<SlotId> {
        if record.is_empty() {
            return Err(DbError::Storage("empty records are not supported".into()));
        }

        let needed = record.len() + SLOT_OVERHEAD;
        if self.free_space() < needed {
            return Err(DbError::Storage("page is full".into()));
        }

        if let Some((slot, existing)) = self
            .slots
            .iter_mut()
            .enumerate()
            .find(|(_, value)| value.is_none())
        {
            *existing = Some(record.to_vec());
            return Ok(SlotId(slot));
        }

        self.slots.push(Some(record.to_vec()));
        Ok(SlotId(self.slots.len() - 1))
    }

    pub fn get(&self, slot_id: SlotId) -> Option<&[u8]> {
        self.slots.get(slot_id.0)?.as_deref()
    }

    pub fn delete(&mut self, slot_id: SlotId) -> Result<()> {
        let slot = self
            .slots
            .get_mut(slot_id.0)
            .ok_or_else(|| DbError::Storage(format!("slot {} not found", slot_id.0)))?;
        *slot = None;
        Ok(())
    }

    pub fn used_space(&self) -> usize {
        self.slots
            .iter()
            .map(|slot| SLOT_OVERHEAD + slot.as_ref().map_or(0, Vec::len))
            .sum()
    }

    pub fn free_space(&self) -> usize {
        PAGE_SIZE.saturating_sub(self.used_space())
    }

    pub fn live_records(&self) -> usize {
        self.slots.iter().filter(|slot| slot.is_some()).count()
    }
}
