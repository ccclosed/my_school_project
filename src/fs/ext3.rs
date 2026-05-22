//! ext2/3 read-only filesystem.
//!
//! ext3 is ext2 + journal. For reading, they are compatible — the journal
//! is only used for crash recovery on write. This implementation ignores
//! the journal and provides read-only access.
//!
//! Assumptions:
//! - 512-byte sectors, 1024-byte blocks (block_size = 2 sectors)
//! - Partition starts at LBA 0 (whole-disk image)
//! - Little-endian

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;
use alloc::format;
use core::fmt;

use super::vfs::{FileSystem, FsError};

// ── Constants ────────────────────────────────────────────────────────────

/// ext2/3/4 magic number in superblock.
const EXT_MAGIC: u16 = 0xEF53;

/// Inode modes.
const S_IFDIR: u16  = 0x4000;
const S_IFREG: u16  = 0x8000;
const S_IFLNK: u16  = 0xA000;
const _S_IFBLK: u16 = 0x6000;
const _S_IFCHR: u16 = 0x2000;

/// Block pointers per inode.
const _DIRECT_BLOCKS: usize = 12;
const INDIRECT_BLOCK: usize = 12;
const DOUBLE_BLOCK: usize  = 13;
const _TRIPLE_BLOCK: usize  = 14;

/// Directory entry file type.
const FT_UNKNOWN: u8  = 0;
const FT_REG: u8      = 1;
const FT_DIR: u8      = 2;
const _FT_CHRDEV: u8  = 3;
const _FT_BLKDEV: u8  = 4;
const _FT_FIFO: u8    = 5;
const _FT_SOCK: u8    = 6;
const FT_SYMLINK: u8  = 7;

// ── On-disk structures ──────────────────────────────────────────────────

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
struct Superblock {
    inodes_count: u32,          // 0x00
    blocks_count: u32,          // 0x04
    r_blocks_count: u32,        // 0x08
    free_blocks_count: u32,     // 0x0C
    free_inodes_count: u32,     // 0x10
    first_data_block: u32,      // 0x14
    log_block_size: u32,        // 0x18
    log_frag_size: u32,         // 0x1C (signed)
    blocks_per_group: u32,      // 0x20
    frags_per_group: u32,       // 0x24
    inodes_per_group: u32,      // 0x28
    mtime: u32,                 // 0x2C
    wtime: u32,                 // 0x30
    mnt_count: u16,             // 0x34
    max_mnt_count: u16,         // 0x36
    magic: u16,                 // 0x38 — must be 0xEF53
    state: u16,                 // 0x3A
    errors: u16,                // 0x3C
    minor_rev_level: u16,       // 0x3E
    lastcheck: u32,             // 0x40
    checkinterval: u32,         // 0x44
    creator_os: u32,            // 0x48
    rev_level: u32,             // 0x4C
    def_resuid: u16,            // 0x50
    def_resgid: u16,            // 0x52
    // … more fields for ext2 rev 1+ (not needed for basic read)
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
struct BgDescriptor {
    block_bitmap: u32,
    inode_bitmap: u32,
    inode_table: u32,
    free_blocks_count: u16,
    free_inodes_count: u16,
    used_dirs_count: u16,
    _pad: u16,
    _reserved: [u8; 12],
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
struct Inode {
    mode: u16,
    uid: u16,
    size: u32,          // lower 32 bits of size
    atime: u32,
    ctime: u32,
    mtime: u32,
    dtime: u32,
    gid: u16,
    links_count: u16,
    sectors: u32,       // 512-byte sector count
    flags: u32,
    _osd1: u32,
    block: [u32; 15],   // 12 direct + 1 indirect + 1 double + 1 triple
    generation: u32,
    file_acl: u32,
    size_high: u32,    // upper 32 bits (ext4: dir acl for ext2 rev 0)
    _faddr: u32,
    // … more fields (ext4 extent tree etc.) — not needed for basic read
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
struct DirEntry {
    inode: u32,
    rec_len: u16,
    name_len: u8,
    file_type: u8,
    // name follows immediately
}

// ── Ext3Fs ───────────────────────────────────────────────────────────────

pub struct Ext3Fs {
    block_size: usize,      // bytes per block
    inodes_per_group: u32,
    blocks_per_group: u32,
    bg_desc_table_block: u32,
    bg_count: u32,
}

impl Ext3Fs {
    /// Mount the filesystem from the disk.
    pub fn mount() -> Result<Self, &'static str> {
        use crate::drivers::ata;

        // Read superblock (block 1, offset 1024 on disk = LBA 2)
        let mut sb_buf = [0u8; 1024];
        ata::read(2, 2, &mut sb_buf).map_err(|_| "ATA read error")?;

        // Superblock is at offset 0 within the 1024-byte buffer
        let sb: &Superblock = unsafe { &*(sb_buf.as_ptr() as *const Superblock) };

        if sb.magic != EXT_MAGIC {
            return Err("not an ext2/3/4 filesystem");
        }

        let block_size: usize = 1024 << sb.log_block_size as usize;
        let blocks_per_group = sb.blocks_per_group;
        let inodes_per_group = sb.inodes_per_group;

        // Block group descriptor table is at the block after the superblock
        let bg_desc_table_block = if block_size == 1024 { 2 } else { 1 };

        let bg_count = (sb.blocks_count + blocks_per_group - 1) / blocks_per_group;

        Ok(Ext3Fs {
            block_size,
            inodes_per_group,
            blocks_per_group,
            bg_desc_table_block,
            bg_count,
        })
    }

    /// Public: block size in bytes.
    pub fn block_size(&self) -> usize {
        self.block_size
    }

    /// Public: list directory entries at `path`.
    pub fn list_dir(&self, path: &str) -> Result<Vec<(String, u32, u8)>, &'static str> {
        let mut buf = vec![0u8; self.block_size];
        let (_ino, inode, _ft) = self.walk(2, path, &mut buf)?;
        if inode.mode & S_IFDIR == 0 {
            return Err("not a directory");
        }
        self.read_dir_inode(&inode)
    }

    /// Public: read file contents at `path`.
    pub fn read_file(&self, path: &str) -> Result<Vec<u8>, &'static str> {
        let mut buf = vec![0u8; self.block_size];
        let (_ino, inode, ft) = self.walk(2, path, &mut buf)?;
        if ft == FT_DIR {
            return Err("is a directory");
        }
        let mut data = Vec::new();
        self.read_file_inode(&inode, &mut data)?;
        Ok(data)
    }

    // ── Write support (ext2 mode, no journal) ─────────────────────────────

    /// Write a filesystem block to disk.
    fn write_block(&self, block: u32, buf: &[u8]) -> Result<(), &'static str> {
        use crate::drivers::ata;
        let sectors_per_block = (self.block_size / 512) as u8;
        let lba = block * sectors_per_block as u32;
        ata::write(lba, sectors_per_block, buf).map_err(|_| "ATA write error")
    }

    /// Read-modify-write a single bit in a bitmap block.
    fn bitmap_set(&self, bitmap_block: u32, bit: u32, value: bool) -> Result<(), &'static str> {
        let mut buf = vec![0u8; self.block_size];
        self.read_block(bitmap_block, &mut buf)?;
        let byte_idx = (bit / 8) as usize;
        let bit_mask = 1u8 << (bit % 8);
        if value {
            buf[byte_idx] |= bit_mask;
        } else {
            buf[byte_idx] &= !bit_mask;
        }
        self.write_block(bitmap_block, &buf)
    }

    /// Find a free bit in a bitmap, mark it used, return the bit index.
    fn bitmap_alloc(&self, bitmap_block: u32, max_bits: u32) -> Result<u32, &'static str> {
        let mut buf = vec![0u8; self.block_size];
        self.read_block(bitmap_block, &mut buf)?;
        for byte_idx in 0..self.block_size {
            let byte = buf[byte_idx];
            if byte != 0xFF {
                for bit in 0..8 {
                    let bit_idx = byte_idx as u32 * 8 + bit;
                    if bit_idx >= max_bits { break; }
                    if byte & (1 << bit) == 0 {
                        buf[byte_idx] |= 1 << bit;
                        self.write_block(bitmap_block, &buf)?;
                        return Ok(bit_idx);
                    }
                }
            }
        }
        Err("no free bits in bitmap")
    }

    /// Allocate a free block. Returns block number.
    fn alloc_block(&self) -> Result<u32, &'static str> {
        let bg = self.read_bg_desc(0, &mut vec![0u8; self.block_size])?;
        let block = self.bitmap_alloc(bg.block_bitmap, self.blocks_per_group)?;
        Ok(block)
    }

    /// Free an allocated block.
    fn free_block(&self, block: u32) -> Result<(), &'static str> {
        let bg = self.read_bg_desc(0, &mut vec![0u8; self.block_size])?;
        self.bitmap_set(bg.block_bitmap, block, false)
    }

    /// Allocate a free inode. Returns inode number.
    fn alloc_inode(&self) -> Result<u32, &'static str> {
        let bg = self.read_bg_desc(0, &mut vec![0u8; self.block_size])?;
        let ino = self.bitmap_alloc(bg.inode_bitmap, self.inodes_per_group)?;
        Ok(ino + 1) // inode numbers start at 1
    }

    /// Free an inode.
    fn free_inode(&self, inode_num: u32) -> Result<(), &'static str> {
        let group = (inode_num - 1) / self.inodes_per_group;
        let idx = (inode_num - 1) % self.inodes_per_group;
        let bg = self.read_bg_desc(group, &mut vec![0u8; self.block_size])?;
        self.bitmap_set(bg.inode_bitmap, idx, false)
    }

    /// Write an inode back to disk.
    fn write_inode(&self, inode_num: u32, inode: &Inode) -> Result<(), &'static str> {
        let group = (inode_num - 1) / self.inodes_per_group;
        let idx = (inode_num - 1) % self.inodes_per_group;
        let bg = self.read_bg_desc(group, &mut vec![0u8; self.block_size])?;
        let inode_table_block = bg.inode_table;
        let inode_size = core::mem::size_of::<Inode>();
        let inodes_per_block = self.block_size() / inode_size;
        let block = inode_table_block + idx / inodes_per_block as u32;

        let mut buf = vec![0u8; self.block_size];
        self.read_block(block, &mut buf)?;
        let offset = (idx % inodes_per_block as u32) as usize * inode_size;
        unsafe {
            core::ptr::copy_nonoverlapping(
                inode as *const Inode as *const u8,
                buf.as_mut_ptr().add(offset),
                inode_size,
            );
        }
        self.write_block(block, &buf)
    }

    /// Write data to a file (overwrite existing data).
    fn write_file_data(&self, inode: &mut Inode, data: &[u8], block_buf: &mut [u8]) -> Result<(), &'static str> {
        let bs = self.block_size();
        let blocks_needed = (data.len() + bs - 1) / bs;

        // Grow file if needed
        for blk_idx in 0..blocks_needed as u32 {
            let mut existing = 0u32;
            if (blk_idx as usize) < 12 {
                existing = inode.block[blk_idx as usize];
            }
            if existing == 0 {
                let new_block = self.alloc_block()?;
                let first_data_block = 1u32; // block 0 is superblock, block 1 is superblock too (for 1024-byte blocks)
                let disk_block = new_block + first_data_block;
                if (blk_idx as usize) < 12 {
                    inode.block[blk_idx as usize] = disk_block;
                } else {
                    return Err("no indirect block write support");
                }
            }
        }

        // Write data
        for blk_idx in 0..blocks_needed as u32 {
            let disk_block = if (blk_idx as usize) < 12 {
                inode.block[blk_idx as usize]
            } else {
                return Err("indirect not writable");
            };
            let offset = blk_idx as usize * bs;
            let copy = (data.len() - offset).min(bs);
            // Fill block buffer with zeros, then copy data
            block_buf[..bs].fill(0);
            block_buf[..copy].copy_from_slice(&data[offset..offset + copy]);
            self.write_block(disk_block, &block_buf[..bs])?;
        }

        // Free excess blocks if file shrank
        let old_blocks = (inode.size as usize + bs - 1) / bs;
        for blk_idx in blocks_needed..old_blocks {
            if blk_idx < 12 && inode.block[blk_idx] != 0 {
                self.free_block(inode.block[blk_idx])?;
                inode.block[blk_idx] = 0;
            }
        }

        inode.size = data.len() as u32;
        Ok(())
    }

    /// Create a directory entry in a directory inode.
    fn dir_add_entry(&self, dir_inode: &mut Inode, name: &str, inode_num: u32, file_type: u8) -> Result<(), &'static str> {
        let mut raw = Vec::new();
        self.read_file_inode(dir_inode, &mut raw)?;

        let entry_size = 8 + name.len();
        let rec_len = ((entry_size + 3) & !3) as u16; // align to 4 bytes
        if rec_len < 12 { return Err("entry too small"); }

        let new_offset = raw.len();
        raw.resize(new_offset + rec_len as usize, 0);

        let de = DirEntry {
            inode: inode_num,
            rec_len,
            name_len: name.len() as u8,
            file_type,
        };
        unsafe {
            core::ptr::copy_nonoverlapping(
                &de as *const DirEntry as *const u8,
                raw.as_mut_ptr().add(new_offset),
                8,
            );
        }
        raw[new_offset + 8..new_offset + 8 + name.len()].copy_from_slice(name.as_bytes());

        // If there's a previous entry, adjust its rec_len to point to this one
        if new_offset > 0 {
            let _prev_de: &mut DirEntry = unsafe { &mut *(raw.as_mut_ptr() as *mut DirEntry) };
            let mut prev_offset = 0usize;
            // Walk to the last entry before new_offset
            loop {
                let de = unsafe { &*(raw.as_ptr().add(prev_offset) as *const DirEntry) };
                if de.rec_len == 0 { break; }
                let next = prev_offset + de.rec_len as usize;
                if next == new_offset {
                    // This is the last entry — adjust its rec_len to span to new_offset
                    let prev: &mut DirEntry = unsafe { &mut *(raw.as_mut_ptr().add(prev_offset) as *mut DirEntry) };
                    prev.rec_len = (new_offset - prev_offset) as u16;
                    break;
                }
                if next > new_offset { break; }
                prev_offset = next;
            }
        }

        let mut block_buf = vec![0u8; self.block_size];
        self.write_file_data(dir_inode, &raw, &mut block_buf)?;
        Ok(())
    }

    /// Remove a directory entry by name.
    fn dir_remove_entry(&self, dir_inode: &mut Inode, name: &str) -> Result<u32, &'static str> {
        let mut raw = Vec::new();
        self.read_file_inode(dir_inode, &mut raw)?;
        let len = dir_inode.size as usize;

        let mut prev_offset = 0usize;
        let mut found_offset = None;

        loop {
            if prev_offset + 8 > len { break; }
            let de: &DirEntry = unsafe { &*(raw.as_ptr().add(prev_offset) as *const DirEntry) };
            if de.inode == 0 || de.rec_len == 0 { break; }
            let name_off = prev_offset + 8;
            let name_end = name_off + de.name_len as usize;
            if name_end <= raw.len() {
                let entry_name = core::str::from_utf8(&raw[name_off..name_end]).unwrap_or("");
                if entry_name == name {
                    found_offset = Some((prev_offset, de.inode));
                    break;
                }
            }
            prev_offset += de.rec_len as usize;
        }

        let (found_at, ino) = found_offset.ok_or("file not found in directory")?;

        // Clear the entry: zero inode field (marks it deleted)
        let de: &mut DirEntry = unsafe { &mut *(raw.as_mut_ptr().add(found_at) as *mut DirEntry) };
        // If this is NOT the last entry, merge its rec_len into the next entry
        let next_offset = found_at + de.rec_len as usize;
        if next_offset < len {
            // Merge: extend previous entry to cover this one's space
            // Find entry that points to `found_at`
            let mut scan = 0usize;
            loop {
                let sde = unsafe { &*(raw.as_ptr().add(scan) as *const DirEntry) };
                if sde.rec_len == 0 { break; }
                let nxt = scan + sde.rec_len as usize;
                if nxt == found_at {
                    let sde_mut: &mut DirEntry = unsafe { &mut *(raw.as_mut_ptr().add(scan) as *mut DirEntry) };
                    sde_mut.rec_len += de.rec_len;
                    break;
                }
                if nxt > found_at { break; }
                scan = nxt;
            }
        }
        de.inode = 0;

        let mut block_buf = vec![0u8; self.block_size];
        self.write_file_data(dir_inode, &raw, &mut block_buf)?;
        Ok(ino)
    }

    /// Create a new file (regular file) in the filesystem.
    pub fn create_file(&self, path: &str) -> Result<(), &'static str> {
        let mut buf = vec![0u8; self.block_size];

        // Walk to parent directory
        let (parent_path, name) = match path.rfind('/') {
            Some(pos) => (&path[..pos], &path[pos+1..]),
            None => return Err("need absolute path"),
        };
        if parent_path.is_empty() { return Err("need absolute path"); }
        let parent = if parent_path == "/" { "/" } else { parent_path };
        if name.is_empty() || name.len() > 255 { return Err("invalid name"); }

        let (parent_ino, mut parent_inode, _) = self.walk(2, parent, &mut buf)?;
        if parent_inode.mode & S_IFDIR == 0 {
            return Err("parent not a directory");
        }

        // Check doesn't already exist
        let entries = self.read_dir_inode(&parent_inode)?;
        for (entry_name, _, _) in &entries {
            if entry_name == name {
                return Err("file exists");
            }
        }

        // Allocate inode
        let ino = self.alloc_inode()?;

        // Create empty inode
        let inode = Inode {
            mode: S_IFREG | 0o644,
            uid: 0, gid: 0,
            size: 0,
            atime: 0, ctime: 0, mtime: 0, dtime: 0,
            links_count: 1,
            sectors: 0,
            flags: 0,
            _osd1: 0,
            block: [0; 15],
            generation: 0,
            file_acl: 0,
            size_high: 0,
            _faddr: 0,
        };
        self.write_inode(ino, &inode)?;

        // Add directory entry
        self.dir_add_entry(&mut parent_inode, name, ino, FT_REG)?;
        self.write_inode(parent_ino, &parent_inode)
    }

    /// Write data to a file (creating it if needed).
    pub fn write_file(&self, path: &str, data: &[u8]) -> Result<(), &'static str> {
        let mut buf = vec![0u8; self.block_size];

        // Try to find existing file
        let (ino, mut inode, ft) = match self.walk(2, path, &mut buf) {
            Ok((i, ino, f)) => (i, ino, f),
            Err(_) => {
                // Create it
                self.create_file(path)?;
                return self.write_file(path, data);
            }
        };

        if ft == FT_DIR {
            return Err("cannot write to directory");
        }

        self.write_file_data(&mut inode, data, &mut buf)?;
        self.write_inode(ino, &inode)
    }

    /// Delete a file.
    pub fn delete_file(&self, path: &str) -> Result<(), &'static str> {
        let mut buf = vec![0u8; self.block_size];

        let (parent_path, name) = match path.rfind('/') {
            Some(pos) => (&path[..pos], &path[pos+1..]),
            None => return Err("need absolute path"),
        };
        if parent_path.is_empty() { return Err("need absolute path"); }
        let parent = if parent_path == "/" { "/" } else { parent_path };

        let (parent_ino, mut parent_inode, _) = self.walk(2, parent, &mut buf)?;
        let (ino, file_inode, _) = self.walk(2, path, &mut buf)?;

        if file_inode.mode & S_IFDIR != 0 {
            return Err("cannot delete directory (not implemented)");
        }

        // Free data blocks
        let bs = self.block_size();
        let blocks = (file_inode.size as usize + bs - 1) / bs;
        for blk_idx in 0..blocks {
            if blk_idx < 12 {
                let blk = file_inode.block[blk_idx];
                if blk != 0 {
                    self.free_block(blk)?;
                }
            }
        }

        // Free inode
        self.free_inode(ino)?;

        // Remove directory entry
        self.dir_remove_entry(&mut parent_inode, name)?;
        self.write_inode(parent_ino, &parent_inode)
    }

    /// Read a filesystem block into `buf`. `buf` must be >= block_size.
    fn read_block(&self, block: u32, buf: &mut [u8]) -> Result<(), &'static str> {
        use crate::drivers::ata;
        let sectors_per_block = (self.block_size / 512) as u8;
        let lba = block * sectors_per_block as u32;
        ata::read(lba, sectors_per_block, buf).map_err(|_| "ATA read error")
    }

    /// Read the block group descriptor for group `group`.
    fn read_bg_desc(&self, group: u32, buf: &mut [u8]) -> Result<BgDescriptor, &'static str> {
        let block_size = self.block_size();
        let gd_per_block = block_size / core::mem::size_of::<BgDescriptor>();
        let gd_block = self.bg_desc_table_block + group / gd_per_block as u32;
        self.read_block(gd_block, buf)?;
        let idx = (group % gd_per_block as u32) as usize;
        let gd: &BgDescriptor = unsafe {
            &*(buf.as_ptr().add(idx * core::mem::size_of::<BgDescriptor>()) as *const BgDescriptor)
        };
        Ok(*gd)
    }

    /// Read an inode by its absolute inode number.
    fn read_inode(&self, inode_num: u32, buf: &mut [u8]) -> Result<Inode, &'static str> {
        let group = (inode_num - 1) / self.inodes_per_group;
        let idx = (inode_num - 1) % self.inodes_per_group;

        let bg = self.read_bg_desc(group, buf)?;
        let inode_table_block = bg.inode_table;
        let inode_size = core::mem::size_of::<Inode>();
        let inodes_per_block = self.block_size() / inode_size;

        let block = inode_table_block + idx / inodes_per_block as u32;
        self.read_block(block, buf)?;

        let offset = (idx % inodes_per_block as u32) as usize * inode_size;
        let inode: &Inode = unsafe { &*(buf.as_ptr().add(offset) as *const Inode) };
        Ok(*inode)
    }

    /// Resolve a block number from an inode (handles direct + indirect).
    fn inode_block(&self, inode: &Inode, blk_idx: u32, buf: &mut [u8]) -> Result<u32, &'static str> {
        let bs = self.block_size();
        if (blk_idx as usize) < 12 {
            return Ok(inode.block[blk_idx as usize]);
        }
        // Single indirect
        if (blk_idx as usize) < 12 + bs / 4 {
            let indirect_blk = inode.block[INDIRECT_BLOCK];
            if indirect_blk == 0 {
                return Err("hole in file");
            }
            self.read_block(indirect_blk, buf)?;
            let idx = blk_idx as usize - 12;
            let blk: &u32 = unsafe { &*(buf.as_ptr().add(idx * 4) as *const u32) };
            return Ok(*blk);
        }
        // Double indirect — not implemented (rarely needed for small files)
        Err("double indirect not supported")
    }

    /// Read file data from an inode into `data`. Returns bytes read.
    fn read_file_inode(&self, inode: &Inode, buf: &mut Vec<u8>) -> Result<usize, &'static str> {
        let size = inode.size as usize;
        let bs = self.block_size();
        let blocks = (size + bs - 1) / bs;

        buf.resize(size, 0);
        let mut block_buf = vec![0u8; bs];

        for blk_idx in 0..blocks as u32 {
            let disk_block = self.inode_block(inode, blk_idx, &mut block_buf)?;
            if disk_block == 0 {
                // Hole — zero-fill
                continue;
            }
            self.read_block(disk_block, &mut block_buf)?;
            let offset = blk_idx as usize * bs;
            let copy = (size - offset).min(bs);
            buf[offset..offset + copy].copy_from_slice(&block_buf[..copy]);
        }

        Ok(size)
    }

    /// List entries in a directory inode.
    fn read_dir_inode(&self, inode: &Inode) -> Result<Vec<(String, u32, u8)>, &'static str> {
        let mut raw = Vec::new();
        self.read_file_inode(inode, &mut raw)?;

        let mut entries = Vec::new();
        let len = inode.size as usize;
        let mut offset = 0usize;

        while offset < len && offset + 8 <= len {
            let de: &DirEntry = unsafe { &*(raw.as_ptr().add(offset) as *const DirEntry) };
            if de.inode == 0 || de.rec_len == 0 {
                break;
            }
            let name_start = offset + 8;
            let name_end = name_start + de.name_len as usize;
            if name_end <= raw.len() {
                let name = core::str::from_utf8(&raw[name_start..name_end])
                    .unwrap_or("<invalid>")
                    .to_string();
                if name != "." && name != ".." {
                    entries.push((name, de.inode, de.file_type));
                }
            }
            offset += de.rec_len as usize;
        }

        Ok(entries)
    }

    /// Walk a path starting from inode `start`. Returns (inode_num, Inode, file_type).
    fn walk(&self, start: u32, path: &str, buf: &mut [u8]) -> Result<(u32, Inode, u8), &'static str> {
        let mut current_inode = start;
        let mut current = self.read_inode(current_inode, buf)?;

        if path.is_empty() || path == "/" {
            let ft = if current.mode & S_IFDIR != 0 { FT_DIR } else { FT_REG };
            return Ok((current_inode, current, ft));
        }

        for component in path.split('/') {
            if component.is_empty() {
                continue;
            }
            // Must be a directory
            if current.mode & S_IFDIR == 0 {
                return Err("not a directory");
            }
            let entries = self.read_dir_inode(&current)?;
            let mut found = None;
            for (name, inode_num, ft) in &entries {
                if name == component {
                    found = Some((*inode_num, *ft));
                    break;
                }
            }
            match found {
                Some((ino, _ft)) => {
                    current_inode = ino;
                    current = self.read_inode(ino, buf)?;
                }
                None => return Err("file not found"),
            }
        }

        let ft = if current.mode & S_IFDIR != 0 { FT_DIR } else { FT_REG };
        Ok((current_inode, current, ft))
    }
}

impl FileSystem for Ext3Fs {
    fn read(&self, path: &str) -> Result<Vec<u8>, FsError> {
        let mut buf = vec![0u8; self.block_size];
        let (_ino, inode, ft) = self.walk(2, path, &mut buf).map_err(|_| FsError::NotFound)?;

        if ft == FT_DIR {
            return Err(FsError::NotFound); // directories don't have "content"
        }

        let mut data = Vec::new();
        self.read_file_inode(&inode, &mut data).map_err(|_| FsError::Io)?;
        Ok(data)
    }

    fn write(&mut self, path: &str, data: &[u8]) -> Result<(), FsError> {
        self.write_file(path, data).map_err(|_| FsError::Io)
    }

    fn create(&mut self, path: &str) -> Result<(), FsError> {
        self.create_file(path).map_err(|_| FsError::Io)
    }

    fn delete(&mut self, path: &str) -> Result<(), FsError> {
        self.delete_file(path).map_err(|_| FsError::Io)
    }

    fn list(&self) -> Vec<(String, usize)> {
        let mut buf = vec![0u8; self.block_size];
        let root_inode = match self.read_inode(2, &mut buf) {
            Ok(i) => i,
            Err(_) => return Vec::new(),
        };
        let entries = match self.read_dir_inode(&root_inode) {
            Ok(e) => e,
            Err(_) => return Vec::new(),
        };
        entries.into_iter().map(|(name, _, ft)| {
            let type_suffix = if ft == FT_DIR { "/" } else { "" };
            (format!("{}{}", name, type_suffix), 0)
        }).collect()
    }
}

impl fmt::Display for Ext3Fs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ext3 (block_size={}, ro)", self.block_size)
    }
}
