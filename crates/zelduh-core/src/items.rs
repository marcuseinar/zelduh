//! Items and the player's inventory.

/// An item that can be equipped to A or B.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[repr(u8)]
pub enum Item {
    #[default]
    None = 0,
    Sword = 1,
    Shield = 2,
    Bombs = 3,
    Bow = 4,
    Boomerang = 5,
    /// Roc's feather: jump over pits and enemies.
    Feather = 6,
    /// Power bracelet: lift rocks as well as bushes.
    Bracelet = 7,
    /// Pegasus boots: hold to dash.
    Boots = 8,
    /// Flippers: swim in deep water.
    Flippers = 9,
}

impl Item {
    pub const ALL: [Item; 9] = [
        Item::Sword,
        Item::Shield,
        Item::Bombs,
        Item::Bow,
        Item::Boomerang,
        Item::Feather,
        Item::Bracelet,
        Item::Boots,
        Item::Flippers,
    ];

    pub fn from_u8(v: u8) -> Item {
        use Item::*;
        match v {
            1 => Sword,
            2 => Shield,
            3 => Bombs,
            4 => Bow,
            5 => Boomerang,
            6 => Feather,
            7 => Bracelet,
            8 => Boots,
            9 => Flippers,
            _ => None,
        }
    }

    /// Short name for the status bar and menus.
    pub fn name(self) -> &'static str {
        use Item::*;
        match self {
            None => "",
            Sword => "SWORD",
            Shield => "SHIELD",
            Bombs => "BOMBS",
            Bow => "BOW",
            Boomerang => "BOOMERANG",
            Feather => "FEATHER",
            Bracelet => "BRACELET",
            Boots => "BOOTS",
            Flippers => "FLIPPERS",
        }
    }

    /// True for items that are simply carried and always in effect.
    pub fn is_passive(self) -> bool {
        matches!(self, Item::Bracelet | Item::Flippers)
    }

    /// How much ammunition using this item costs.
    pub fn ammo_cost(self) -> u8 {
        match self {
            Item::Bombs | Item::Bow => 1,
            _ => 0,
        }
    }
}

/// Everything a player is carrying.
#[derive(Clone, Debug)]
pub struct Inventory {
    /// Bit per [`Item`] discriminant.
    owned: u32,
    /// Items bound to the A and B buttons.
    pub equipped: [Item; 2],
    pub bombs: u8,
    pub max_bombs: u8,
    pub arrows: u8,
    pub max_arrows: u8,
    pub keys: u8,
    pub rupees: u16,
    /// Sword upgrade level, 1 or 2.
    pub sword_level: u8,
    /// Heart containers collected, in quarter-heart pieces.
    pub heart_pieces: u8,
}

impl Default for Inventory {
    fn default() -> Self {
        let mut inv = Inventory {
            owned: 0,
            equipped: [Item::Sword, Item::None],
            bombs: 0,
            max_bombs: 30,
            arrows: 0,
            max_arrows: 30,
            keys: 0,
            rupees: 0,
            sword_level: 1,
            heart_pieces: 0,
        };
        inv.give(Item::Sword);
        inv
    }
}

impl Inventory {
    /// The raw owned-items bitmask, for snapshots.
    pub fn owned_bits(&self) -> u32 {
        self.owned
    }

    /// Restores the owned-items bitmask from a snapshot.
    pub fn set_owned_bits(&mut self, bits: u32) {
        self.owned = bits;
    }

    /// Grants an item.
    pub fn give(&mut self, item: Item) {
        if item != Item::None {
            self.owned |= 1 << (item as u32);
            // A newly found active item goes straight to the free button so it
            // can be used without opening a menu.
            if !item.is_passive() && self.equipped[1] == Item::None && item != self.equipped[0] {
                self.equipped[1] = item;
            }
        }
    }

    /// True when the player owns the item.
    pub fn has(&self, item: Item) -> bool {
        item != Item::None && self.owned & (1 << (item as u32)) != 0
    }

    /// Owned items in discriminant order.
    pub fn owned_items(&self) -> impl Iterator<Item = Item> + '_ {
        Item::ALL.into_iter().filter(|i| self.has(*i))
    }

    /// True when the item can be used right now, i.e. owned and with ammo.
    pub fn can_use(&self, item: Item) -> bool {
        if !self.has(item) {
            return false;
        }
        match item {
            Item::Bombs => self.bombs > 0,
            Item::Bow => self.arrows > 0,
            _ => true,
        }
    }

    /// Spends one unit of ammunition. Returns false if there was none.
    pub fn spend(&mut self, item: Item) -> bool {
        match item {
            Item::Bombs if self.bombs > 0 => {
                self.bombs -= 1;
                true
            }
            Item::Bow if self.arrows > 0 => {
                self.arrows -= 1;
                true
            }
            Item::Bombs | Item::Bow => false,
            _ => true,
        }
    }

    pub fn add_bombs(&mut self, n: u8) {
        self.bombs = (self.bombs.saturating_add(n)).min(self.max_bombs);
    }

    pub fn add_arrows(&mut self, n: u8) {
        self.arrows = (self.arrows.saturating_add(n)).min(self.max_arrows);
    }

    pub fn add_rupees(&mut self, n: u16) {
        self.rupees = (self.rupees.saturating_add(n)).min(999);
    }

    /// Spends a small key, returning false when the player has none.
    pub fn use_key(&mut self) -> bool {
        if self.keys > 0 {
            self.keys -= 1;
            true
        } else {
            false
        }
    }

    /// Swaps the A and B assignments.
    pub fn swap_equipped(&mut self) {
        self.equipped.swap(0, 1);
    }

    /// Cycles the given button through the owned active items.
    pub fn cycle_equipped(&mut self, slot: usize) {
        let usable: Vec<Item> = self.owned_items().filter(|i| !i.is_passive()).collect();
        if usable.is_empty() {
            return;
        }
        let slot = slot & 1;
        let cur = self.equipped[slot];
        let start = usable
            .iter()
            .position(|i| *i == cur)
            .map(|p| p + 1)
            .unwrap_or(0);
        for n in 0..usable.len() {
            let cand = usable[(start + n) % usable.len()];
            if cand != self.equipped[slot ^ 1] {
                self.equipped[slot] = cand;
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_with_a_sword_equipped() {
        let inv = Inventory::default();
        assert!(inv.has(Item::Sword));
        assert_eq!(inv.equipped[0], Item::Sword);
        assert!(!inv.has(Item::Bombs));
    }

    #[test]
    fn bombs_need_ammo() {
        let mut inv = Inventory::default();
        inv.give(Item::Bombs);
        assert!(inv.has(Item::Bombs));
        assert!(!inv.can_use(Item::Bombs), "no bombs in the bag yet");
        inv.add_bombs(3);
        assert!(inv.can_use(Item::Bombs));
        assert!(inv.spend(Item::Bombs));
        assert_eq!(inv.bombs, 2);
    }

    #[test]
    fn a_new_item_auto_equips_to_the_free_button() {
        let mut inv = Inventory::default();
        inv.give(Item::Bombs);
        assert_eq!(inv.equipped[1], Item::Bombs);
        inv.give(Item::Bow);
        assert_eq!(inv.equipped[1], Item::Bombs, "the button is taken now");
    }

    #[test]
    fn cycling_never_equips_the_same_item_twice() {
        let mut inv = Inventory::default();
        inv.give(Item::Bombs);
        inv.give(Item::Bow);
        inv.give(Item::Boomerang);
        for _ in 0..12 {
            inv.cycle_equipped(1);
            assert_ne!(inv.equipped[0], inv.equipped[1]);
        }
    }

    #[test]
    fn passive_items_are_never_equipped() {
        let mut inv = Inventory::default();
        inv.equipped[1] = Item::None;
        inv.give(Item::Flippers);
        assert_eq!(inv.equipped[1], Item::None);
        assert!(inv.has(Item::Flippers));
    }

    #[test]
    fn rupees_cap_at_999() {
        let mut inv = Inventory::default();
        inv.add_rupees(60000);
        assert_eq!(inv.rupees, 999);
    }
}
