// capabilities. seL4 风格: 扁平 slot 表 + 派生树, revoke 级联
// slot 0 永远空着, 0 当 NULL 用

pub const CAP_ENDPOINT: u32 = 1;
pub const CAP_VMO: u32 = 2;
pub const CAP_TASK: u32 = 3;

pub const NO_SLOT: u32 = u32::MAX;

#[derive(Clone, Copy)]
pub struct CapNode {
    pub obj_type: u32,
    pub obj_id: u32,
    pub parent: u32,
    pub first_child: u32,
    pub next_sibling: u32,
}

pub struct CSpace {
    nodes: *mut CapNode,
    used: *mut bool,
    free_stack: *mut u32,
    total: usize,
    top: usize, // free stack 栈顶
    live: usize,
}

impl CSpace {
    pub fn new(nodes: *mut CapNode, used: *mut bool, free_stack: *mut u32, total: usize) -> CSpace {
        unsafe {
            for i in 0..total {
                *used.add(i) = false;
                *free_stack.add(i) = i as u32;
            }
        }
        CSpace { nodes, used, free_stack, total, top: total, live: 0 }
    }

    fn alloc_slot(&mut self) -> Option<u32> {
        if self.top == 0 {
            return None;
        }
        self.top -= 1;
        Some(unsafe { *self.free_stack.add(self.top) })
    }

    fn release_slot(&mut self, s: u32) {
        unsafe {
            *self.used.add(s as usize) = false;
            *self.free_stack.add(self.top) = s;
        }
        self.top += 1;
        self.live -= 1;
    }

    // parent = NO_SLOT 就是凭空 mint 一根
    pub fn grant(&mut self, obj_type: u32, obj_id: u32, parent: u32) -> u32 {
        if parent != NO_SLOT && unsafe { !*self.used.add(parent as usize) } {
            return NO_SLOT;
        }
        let s = match self.alloc_slot() {
            Some(x) => x,
            None => return NO_SLOT,
        };
        unsafe {
            let n = self.nodes.add(s as usize);
            *n = CapNode { obj_type, obj_id, parent, first_child: NO_SLOT, next_sibling: NO_SLOT };
            if parent != NO_SLOT {
                let p = self.nodes.add(parent as usize);
                (*n).next_sibling = (*p).first_child;
                (*p).first_child = s;
            }
            *self.used.add(s as usize) = true;
        }
        self.live += 1;
        s
    }

    pub fn derive(&mut self, parent: u32) -> u32 {
        if parent == NO_SLOT || unsafe { !*self.used.add(parent as usize) } {
            return NO_SLOT;
        }
        let (t, id) = unsafe {
            let p = self.nodes.add(parent as usize);
            ((*p).obj_type, (*p).obj_id)
        };
        self.grant(t, id, parent)
    }

    pub fn lookup(&self, s: u32) -> Option<(u32, u32)> {
        if s == NO_SLOT || s as usize >= self.total {
            return None;
        }
        if unsafe { !*self.used.add(s as usize) } {
            return None;
        }
        let n = unsafe { *self.nodes.add(s as usize) };
        Some((n.obj_type, n.obj_id))
    }

    // seL4 语义: 杀光后代, 自己留着
    pub fn revoke(&mut self, s: u32) -> usize {
        if s == NO_SLOT || s as usize >= self.total {
            return 0;
        }
        if unsafe { !*self.used.add(s as usize) } {
            return 0;
        }
        let first = unsafe { (*self.nodes.add(s as usize)).first_child };
        unsafe { (*self.nodes.add(s as usize)).first_child = NO_SLOT; }
        let mut n = 0;
        self.kill_chain(first, &mut n);
        n
    }

    fn kill_chain(&mut self, s: u32, n: &mut usize) {
        if s == NO_SLOT {
            return;
        }
        let node = unsafe { *self.nodes.add(s as usize) };
        self.kill_chain(node.next_sibling, n);
        self.kill_chain(node.first_child, n);
        self.release_slot(s);
        *n += 1;
    }

    pub fn live_count(&self) -> usize {
        self.live
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cs(n: usize) -> (Vec<CapNode>, Vec<bool>, Vec<u32>, CSpace) {
        let mut nodes = vec![CapNode { obj_type: 0, obj_id: 0, parent: 0, first_child: NO_SLOT, next_sibling: NO_SLOT }; n];
        let mut used = vec![false; n];
        let mut stack = vec![0u32; n];
        let c = CSpace::new(nodes.as_mut_ptr(), used.as_mut_ptr(), stack.as_mut_ptr(), n);
        (nodes, used, stack, c)
    }

    #[test]
    fn grant_lookup_basic() {
        let (_a, _b, _c, mut c) = cs(16);
        let s = c.grant(CAP_VMO, 42, NO_SLOT);
        assert_ne!(s, NO_SLOT);
        assert_eq!(c.lookup(s), Some((CAP_VMO, 42)));
        assert_eq!(c.lookup(NO_SLOT), None);
        assert_eq!(c.lookup(99), None);
    }

    #[test]
    fn derive_and_revoke_cascade() {
        let (_a, _b, _c, mut c) = cs(16);
        let root = c.grant(CAP_ENDPOINT, 7, NO_SLOT);
        let child = c.derive(root);
        let grand = c.derive(child);
        assert_ne!(child, NO_SLOT);
        assert_ne!(grand, NO_SLOT);
        assert_eq!(c.live_count(), 3);

        let killed = c.revoke(root);
        assert_eq!(killed, 2);
        assert_eq!(c.lookup(child), None);
        assert_eq!(c.lookup(grand), None);
        assert_eq!(c.lookup(root), Some((CAP_ENDPOINT, 7)));
        assert_eq!(c.live_count(), 1);
    }

    #[test]
    fn revoke_middle_only_kills_below() {
        let (_a, _b, _c, mut c) = cs(16);
        let root = c.grant(CAP_VMO, 1, NO_SLOT);
        let a = c.derive(root);
        let b = c.derive(root);
        let a2 = c.derive(a);

        assert_eq!(c.revoke(a), 1);
        assert_eq!(c.lookup(a2), None);
        assert_eq!(c.lookup(b), Some((CAP_VMO, 1)));
        assert_eq!(c.lookup(root), Some((CAP_VMO, 1)));
    }

    #[test]
    fn slots_recycled() {
        let (_a, _b, _c, mut c) = cs(4);
        let r = c.grant(CAP_TASK, 1, NO_SLOT);
        let _x = c.derive(r);
        c.revoke(r);
        let y = c.grant(CAP_TASK, 2, NO_SLOT);
        assert_ne!(y, NO_SLOT);
        assert_eq!(c.live_count(), 2); // r 还活着 + y
        assert_eq!(c.lookup(y), Some((CAP_TASK, 2)));
    }

    #[test]
    fn exhaustion() {
        let (_a, _b, _c, mut c) = cs(2);
        assert_ne!(c.grant(CAP_VMO, 0, NO_SLOT), NO_SLOT);
        assert_ne!(c.grant(CAP_VMO, 0, NO_SLOT), NO_SLOT);
        assert_eq!(c.grant(CAP_VMO, 0, NO_SLOT), NO_SLOT);
    }

    #[test]
    fn derive_from_dead_parent() {
        let (_a, _b, _c, mut c) = cs(8);
        let r = c.grant(CAP_VMO, 5, NO_SLOT);
        let x = c.derive(r);
        c.revoke(r);
        assert_eq!(c.derive(x), NO_SLOT);
        assert_eq!(c.grant(CAP_VMO, 5, x), NO_SLOT);
    }
}
