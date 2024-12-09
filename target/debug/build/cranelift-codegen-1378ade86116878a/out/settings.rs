#[derive(Clone, Hash)]
/// Flags group `shared`.
pub struct Flags {
    bytes: [u8; 9],
}
impl Flags {
    /// Create flags shared settings group.
    #[allow(unused_variables)]
    pub fn new(builder: Builder) -> Self {
        let bvec = builder.state_for("shared");
        let mut shared = Self { bytes: [0; 9] };
        debug_assert_eq!(bvec.len(), 9);
        shared.bytes[0..9].copy_from_slice(&bvec);
        shared
    }
}
impl Flags {
    /// Iterates the setting values.
    pub fn iter(&self) -> impl Iterator<Item = Value> {
        let mut bytes = [0; 9];
        bytes.copy_from_slice(&self.bytes[0..9]);
        DESCRIPTORS.iter().filter_map(move |d| {
            let values = match &d.detail {
                detail::Detail::Preset => return None,
                detail::Detail::Enum { last, enumerators } => Some(TEMPLATE.enums(*last, *enumerators)),
                _ => None
            };
            Some(Value{ name: d.name, detail: d.detail, values, value: bytes[d.offset as usize] })
        })
    }
}
/// Values for `shared.opt_level`.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum OptLevel {
    /// `none`.
    None,
    /// `speed`.
    Speed,
    /// `speed_and_size`.
    SpeedAndSize,
}
impl OptLevel {
    /// Returns a slice with all possible [OptLevel] values.
    pub fn all() -> &'static [OptLevel] {
        &[
            Self::None,
            Self::Speed,
            Self::SpeedAndSize,
        ]
    }
}
impl fmt::Display for OptLevel {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(match *self {
            Self::None => "none",
            Self::Speed => "speed",
            Self::SpeedAndSize => "speed_and_size",
        })
    }
}
impl core::str::FromStr for OptLevel {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "none" => Ok(Self::None),
            "speed" => Ok(Self::Speed),
            "speed_and_size" => Ok(Self::SpeedAndSize),
            _ => Err(()),
        }
    }
}
/// Values for `shared.tls_model`.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum TlsModel {
    /// `none`.
    None,
    /// `elf_gd`.
    ElfGd,
    /// `macho`.
    Macho,
    /// `coff`.
    Coff,
}
impl TlsModel {
    /// Returns a slice with all possible [TlsModel] values.
    pub fn all() -> &'static [TlsModel] {
        &[
            Self::None,
            Self::ElfGd,
            Self::Macho,
            Self::Coff,
        ]
    }
}
impl fmt::Display for TlsModel {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(match *self {
            Self::None => "none",
            Self::ElfGd => "elf_gd",
            Self::Macho => "macho",
            Self::Coff => "coff",
        })
    }
}
impl core::str::FromStr for TlsModel {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "none" => Ok(Self::None),
            "elf_gd" => Ok(Self::ElfGd),
            "macho" => Ok(Self::Macho),
            "coff" => Ok(Self::Coff),
            _ => Err(()),
        }
    }
}
/// Values for `shared.libcall_call_conv`.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum LibcallCallConv {
    /// `isa_default`.
    IsaDefault,
    /// `fast`.
    Fast,
    /// `cold`.
    Cold,
    /// `system_v`.
    SystemV,
    /// `windows_fastcall`.
    WindowsFastcall,
    /// `apple_aarch64`.
    AppleAarch64,
    /// `probestack`.
    Probestack,
}
impl LibcallCallConv {
    /// Returns a slice with all possible [LibcallCallConv] values.
    pub fn all() -> &'static [LibcallCallConv] {
        &[
            Self::IsaDefault,
            Self::Fast,
            Self::Cold,
            Self::SystemV,
            Self::WindowsFastcall,
            Self::AppleAarch64,
            Self::Probestack,
        ]
    }
}
impl fmt::Display for LibcallCallConv {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(match *self {
            Self::IsaDefault => "isa_default",
            Self::Fast => "fast",
            Self::Cold => "cold",
            Self::SystemV => "system_v",
            Self::WindowsFastcall => "windows_fastcall",
            Self::AppleAarch64 => "apple_aarch64",
            Self::Probestack => "probestack",
        })
    }
}
impl core::str::FromStr for LibcallCallConv {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "isa_default" => Ok(Self::IsaDefault),
            "fast" => Ok(Self::Fast),
            "cold" => Ok(Self::Cold),
            "system_v" => Ok(Self::SystemV),
            "windows_fastcall" => Ok(Self::WindowsFastcall),
            "apple_aarch64" => Ok(Self::AppleAarch64),
            "probestack" => Ok(Self::Probestack),
            _ => Err(()),
        }
    }
}
/// Values for `shared.probestack_strategy`.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum ProbestackStrategy {
    /// `outline`.
    Outline,
    /// `inline`.
    Inline,
}
impl ProbestackStrategy {
    /// Returns a slice with all possible [ProbestackStrategy] values.
    pub fn all() -> &'static [ProbestackStrategy] {
        &[
            Self::Outline,
            Self::Inline,
        ]
    }
}
impl fmt::Display for ProbestackStrategy {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(match *self {
            Self::Outline => "outline",
            Self::Inline => "inline",
        })
    }
}
impl core::str::FromStr for ProbestackStrategy {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "outline" => Ok(Self::Outline),
            "inline" => Ok(Self::Inline),
            _ => Err(()),
        }
    }
}
/// User-defined settings.
#[allow(dead_code)]
impl Flags {
    /// Get a view of the boolean predicates.
    pub fn predicate_view(&self) -> crate::settings::PredicateView {
        crate::settings::PredicateView::new(&self.bytes[6..])
    }
    /// Dynamic numbered predicate getter.
    fn numbered_predicate(&self, p: usize) -> bool {
        self.bytes[6 + p / 8] & (1 << (p % 8)) != 0
    }
    /// Optimization level for generated code.
    ///
    /// Supported levels:
    ///
    /// - `none`: Minimise compile time by disabling most optimizations.
    /// - `speed`: Generate the fastest possible code
    /// - `speed_and_size`: like "speed", but also perform transformations aimed at reducing code size.
    pub fn opt_level(&self) -> OptLevel {
        match self.bytes[0] {
            0 => {
                OptLevel::None
            }
            1 => {
                OptLevel::Speed
            }
            2 => {
                OptLevel::SpeedAndSize
            }
            _ => {
                panic!("Invalid enum value")
            }
        }
    }
    /// Defines the model used to perform TLS accesses.
    pub fn tls_model(&self) -> TlsModel {
        match self.bytes[1] {
            3 => {
                TlsModel::Coff
            }
            1 => {
                TlsModel::ElfGd
            }
            2 => {
                TlsModel::Macho
            }
            0 => {
                TlsModel::None
            }
            _ => {
                panic!("Invalid enum value")
            }
        }
    }
    /// Defines the calling convention to use for LibCalls call expansion.
    ///
    /// This may be different from the ISA default calling convention.
    ///
    /// The default value is to use the same calling convention as the ISA
    /// default calling convention.
    ///
    /// This list should be kept in sync with the list of calling
    /// conventions available in isa/call_conv.rs.
    pub fn libcall_call_conv(&self) -> LibcallCallConv {
        match self.bytes[2] {
            5 => {
                LibcallCallConv::AppleAarch64
            }
            2 => {
                LibcallCallConv::Cold
            }
            1 => {
                LibcallCallConv::Fast
            }
            0 => {
                LibcallCallConv::IsaDefault
            }
            6 => {
                LibcallCallConv::Probestack
            }
            3 => {
                LibcallCallConv::SystemV
            }
            4 => {
                LibcallCallConv::WindowsFastcall
            }
            _ => {
                panic!("Invalid enum value")
            }
        }
    }
    /// The log2 of the size of the stack guard region.
    ///
    /// Stack frames larger than this size will have stack overflow checked
    /// by calling the probestack function.
    ///
    /// The default is 12, which translates to a size of 4096.
    pub fn probestack_size_log2(&self) -> u8 {
        self.bytes[3]
    }
    /// Controls what kinds of stack probes are emitted.
    ///
    /// Supported strategies:
    ///
    /// - `outline`: Always emits stack probes as calls to a probe stack function.
    /// - `inline`: Always emits inline stack probes.
    pub fn probestack_strategy(&self) -> ProbestackStrategy {
        match self.bytes[4] {
            1 => {
                ProbestackStrategy::Inline
            }
            0 => {
                ProbestackStrategy::Outline
            }
            _ => {
                panic!("Invalid enum value")
            }
        }
    }
    /// The log2 of the size to insert dummy padding between basic blocks
    ///
    /// This is a debugging option for stressing various cases during code
    /// generation without requiring large functions. This will insert
    /// 0-byte padding between basic blocks of the specified size.
    ///
    /// The amount of padding inserted two raised to the power of this value
    /// minus one. If this value is 0 then no padding is inserted.
    ///
    /// The default for this option is 0 to insert no padding as it's only
    /// intended for testing and development.
    pub fn bb_padding_log2_minus_one(&self) -> u8 {
        self.bytes[5]
    }
    /// Enable the symbolic checker for register allocation.
    ///
    /// This performs a verification that the register allocator preserves
    /// equivalent dataflow with respect to the original (pre-regalloc)
    /// program. This analysis is somewhat expensive. However, if it succeeds,
    /// it provides independent evidence (by a carefully-reviewed, from-first-principles
    /// analysis) that no regalloc bugs were triggered for the particular compilations
    /// performed. This is a valuable assurance to have as regalloc bugs can be
    /// very dangerous and difficult to debug.
    pub fn regalloc_checker(&self) -> bool {
        self.numbered_predicate(0)
    }
    /// Enable verbose debug logs for regalloc2.
    ///
    /// This adds extra logging for regalloc2 output, that is quite valuable to understand
    /// decisions taken by the register allocator as well as debugging it. It is disabled by
    /// default, as it can cause many log calls which can slow down compilation by a large
    /// amount.
    pub fn regalloc_verbose_logs(&self) -> bool {
        self.numbered_predicate(1)
    }
    /// Do redundant-load optimizations with alias analysis.
    ///
    /// This enables the use of a simple alias analysis to optimize away redundant loads.
    /// Only effective when `opt_level` is `speed` or `speed_and_size`.
    pub fn enable_alias_analysis(&self) -> bool {
        self.numbered_predicate(2)
    }
    /// Run the Cranelift IR verifier at strategic times during compilation.
    ///
    /// This makes compilation slower but catches many bugs. The verifier is always enabled by
    /// default, which is useful during development.
    pub fn enable_verifier(&self) -> bool {
        self.numbered_predicate(3)
    }
    /// Enable proof-carrying code translation validation.
    ///
    /// This adds a proof-carrying-code mode. Proof-carrying code (PCC) is a strategy to verify
    /// that the compiler preserves certain properties or invariants in the compiled code.
    /// For example, a frontend that translates WebAssembly to CLIF can embed PCC facts in
    /// the CLIF, and Cranelift will verify that the final machine code satisfies the stated
    /// facts at each intermediate computed value. Loads and stores can be marked as "checked"
    /// and their memory effects can be verified as safe.
    pub fn enable_pcc(&self) -> bool {
        self.numbered_predicate(4)
    }
    /// Enable Position-Independent Code generation.
    pub fn is_pic(&self) -> bool {
        self.numbered_predicate(5)
    }
    /// Use colocated libcalls.
    ///
    /// Generate code that assumes that libcalls can be declared "colocated",
    /// meaning they will be defined along with the current function, such that
    /// they can use more efficient addressing.
    pub fn use_colocated_libcalls(&self) -> bool {
        self.numbered_predicate(6)
    }
    /// Enable the use of floating-point instructions.
    ///
    /// Disabling use of floating-point instructions is not yet implemented.
    pub fn enable_float(&self) -> bool {
        self.numbered_predicate(7)
    }
    /// Enable NaN canonicalization.
    ///
    /// This replaces NaNs with a single canonical value, for users requiring
    /// entirely deterministic WebAssembly computation. This is not required
    /// by the WebAssembly spec, so it is not enabled by default.
    pub fn enable_nan_canonicalization(&self) -> bool {
        self.numbered_predicate(8)
    }
    /// Enable the use of the pinned register.
    ///
    /// This register is excluded from register allocation, and is completely under the control of
    /// the end-user. It is possible to read it via the get_pinned_reg instruction, and to set it
    /// with the set_pinned_reg instruction.
    pub fn enable_pinned_reg(&self) -> bool {
        self.numbered_predicate(9)
    }
    /// Enable the use of atomic instructions
    pub fn enable_atomics(&self) -> bool {
        self.numbered_predicate(10)
    }
    /// Enable safepoint instruction insertions.
    ///
    /// This will allow the emit_stack_maps() function to insert the safepoint
    /// instruction on top of calls and interrupt traps in order to display the
    /// live reference values at that point in the program.
    pub fn enable_safepoints(&self) -> bool {
        self.numbered_predicate(11)
    }
    /// Enable various ABI extensions defined by LLVM's behavior.
    ///
    /// In some cases, LLVM's implementation of an ABI (calling convention)
    /// goes beyond a standard and supports additional argument types or
    /// behavior. This option instructs Cranelift codegen to follow LLVM's
    /// behavior where applicable.
    ///
    /// Currently, this applies only to Windows Fastcall on x86-64, and
    /// allows an `i128` argument to be spread across two 64-bit integer
    /// registers. The Fastcall implementation otherwise does not support
    /// `i128` arguments, and will panic if they are present and this
    /// option is not set.
    pub fn enable_llvm_abi_extensions(&self) -> bool {
        self.numbered_predicate(12)
    }
    /// Generate unwind information.
    ///
    /// This increases metadata size and compile time, but allows for the
    /// debugger to trace frames, is needed for GC tracing that relies on
    /// libunwind (such as in Wasmtime), and is unconditionally needed on
    /// certain platforms (such as Windows) that must always be able to unwind.
    pub fn unwind_info(&self) -> bool {
        self.numbered_predicate(13)
    }
    /// Preserve frame pointers
    ///
    /// Preserving frame pointers -- even inside leaf functions -- makes it
    /// easy to capture the stack of a running program, without requiring any
    /// side tables or metadata (like `.eh_frame` sections). Many sampling
    /// profilers and similar tools walk frame pointers to capture stacks.
    /// Enabling this option will play nice with those tools.
    pub fn preserve_frame_pointers(&self) -> bool {
        self.numbered_predicate(14)
    }
    /// Generate CFG metadata for machine code.
    ///
    /// This increases metadata size and compile time, but allows for the
    /// embedder to more easily post-process or analyze the generated
    /// machine code. It provides code offsets for the start of each
    /// basic block in the generated machine code, and a list of CFG
    /// edges (with blocks identified by start offsets) between them.
    /// This is useful for, e.g., machine-code analyses that verify certain
    /// properties of the generated code.
    pub fn machine_code_cfg_info(&self) -> bool {
        self.numbered_predicate(15)
    }
    /// Enable the use of stack probes for supported calling conventions.
    pub fn enable_probestack(&self) -> bool {
        self.numbered_predicate(16)
    }
    /// Enable if the stack probe adjusts the stack pointer.
    pub fn probestack_func_adjusts_sp(&self) -> bool {
        self.numbered_predicate(17)
    }
    /// Enable the use of jump tables in generated machine code.
    pub fn enable_jump_tables(&self) -> bool {
        self.numbered_predicate(18)
    }
    /// Enable Spectre mitigation on heap bounds checks.
    ///
    /// This is a no-op for any heap that needs no bounds checks; e.g.,
    /// if the limit is static and the guard region is large enough that
    /// the index cannot reach past it.
    ///
    /// This option is enabled by default because it is highly
    /// recommended for secure sandboxing. The embedder should consider
    /// the security implications carefully before disabling this option.
    pub fn enable_heap_access_spectre_mitigation(&self) -> bool {
        self.numbered_predicate(19)
    }
    /// Enable Spectre mitigation on table bounds checks.
    ///
    /// This option uses a conditional move to ensure that when a table
    /// access index is bounds-checked and a conditional branch is used
    /// for the out-of-bounds case, a misspeculation of that conditional
    /// branch (falsely predicted in-bounds) will select an in-bounds
    /// index to load on the speculative path.
    ///
    /// This option is enabled by default because it is highly
    /// recommended for secure sandboxing. The embedder should consider
    /// the security implications carefully before disabling this option.
    pub fn enable_table_access_spectre_mitigation(&self) -> bool {
        self.numbered_predicate(20)
    }
    /// Enable additional checks for debugging the incremental compilation cache.
    ///
    /// Enables additional checks that are useful during development of the incremental
    /// compilation cache. This should be mostly useful for Cranelift hackers, as well as for
    /// helping to debug false incremental cache positives for embedders.
    ///
    /// This option is disabled by default and requires enabling the "incremental-cache" Cargo
    /// feature in cranelift-codegen.
    pub fn enable_incremental_compilation_cache_checks(&self) -> bool {
        self.numbered_predicate(21)
    }
}
static DESCRIPTORS: [detail::Descriptor; 28] = [
    detail::Descriptor {
        name: "opt_level",
        description: "Optimization level for generated code.",
        offset: 0,
        detail: detail::Detail::Enum { last: 2, enumerators: 0 },
    },
    detail::Descriptor {
        name: "tls_model",
        description: "Defines the model used to perform TLS accesses.",
        offset: 1,
        detail: detail::Detail::Enum { last: 3, enumerators: 3 },
    },
    detail::Descriptor {
        name: "libcall_call_conv",
        description: "Defines the calling convention to use for LibCalls call expansion.",
        offset: 2,
        detail: detail::Detail::Enum { last: 6, enumerators: 7 },
    },
    detail::Descriptor {
        name: "probestack_size_log2",
        description: "The log2 of the size of the stack guard region.",
        offset: 3,
        detail: detail::Detail::Num,
    },
    detail::Descriptor {
        name: "probestack_strategy",
        description: "Controls what kinds of stack probes are emitted.",
        offset: 4,
        detail: detail::Detail::Enum { last: 1, enumerators: 14 },
    },
    detail::Descriptor {
        name: "bb_padding_log2_minus_one",
        description: "The log2 of the size to insert dummy padding between basic blocks",
        offset: 5,
        detail: detail::Detail::Num,
    },
    detail::Descriptor {
        name: "regalloc_checker",
        description: "Enable the symbolic checker for register allocation.",
        offset: 6,
        detail: detail::Detail::Bool { bit: 0 },
    },
    detail::Descriptor {
        name: "regalloc_verbose_logs",
        description: "Enable verbose debug logs for regalloc2.",
        offset: 6,
        detail: detail::Detail::Bool { bit: 1 },
    },
    detail::Descriptor {
        name: "enable_alias_analysis",
        description: "Do redundant-load optimizations with alias analysis.",
        offset: 6,
        detail: detail::Detail::Bool { bit: 2 },
    },
    detail::Descriptor {
        name: "enable_verifier",
        description: "Run the Cranelift IR verifier at strategic times during compilation.",
        offset: 6,
        detail: detail::Detail::Bool { bit: 3 },
    },
    detail::Descriptor {
        name: "enable_pcc",
        description: "Enable proof-carrying code translation validation.",
        offset: 6,
        detail: detail::Detail::Bool { bit: 4 },
    },
    detail::Descriptor {
        name: "is_pic",
        description: "Enable Position-Independent Code generation.",
        offset: 6,
        detail: detail::Detail::Bool { bit: 5 },
    },
    detail::Descriptor {
        name: "use_colocated_libcalls",
        description: "Use colocated libcalls.",
        offset: 6,
        detail: detail::Detail::Bool { bit: 6 },
    },
    detail::Descriptor {
        name: "enable_float",
        description: "Enable the use of floating-point instructions.",
        offset: 6,
        detail: detail::Detail::Bool { bit: 7 },
    },
    detail::Descriptor {
        name: "enable_nan_canonicalization",
        description: "Enable NaN canonicalization.",
        offset: 7,
        detail: detail::Detail::Bool { bit: 0 },
    },
    detail::Descriptor {
        name: "enable_pinned_reg",
        description: "Enable the use of the pinned register.",
        offset: 7,
        detail: detail::Detail::Bool { bit: 1 },
    },
    detail::Descriptor {
        name: "enable_atomics",
        description: "Enable the use of atomic instructions",
        offset: 7,
        detail: detail::Detail::Bool { bit: 2 },
    },
    detail::Descriptor {
        name: "enable_safepoints",
        description: "Enable safepoint instruction insertions.",
        offset: 7,
        detail: detail::Detail::Bool { bit: 3 },
    },
    detail::Descriptor {
        name: "enable_llvm_abi_extensions",
        description: "Enable various ABI extensions defined by LLVM's behavior.",
        offset: 7,
        detail: detail::Detail::Bool { bit: 4 },
    },
    detail::Descriptor {
        name: "unwind_info",
        description: "Generate unwind information.",
        offset: 7,
        detail: detail::Detail::Bool { bit: 5 },
    },
    detail::Descriptor {
        name: "preserve_frame_pointers",
        description: "Preserve frame pointers",
        offset: 7,
        detail: detail::Detail::Bool { bit: 6 },
    },
    detail::Descriptor {
        name: "machine_code_cfg_info",
        description: "Generate CFG metadata for machine code.",
        offset: 7,
        detail: detail::Detail::Bool { bit: 7 },
    },
    detail::Descriptor {
        name: "enable_probestack",
        description: "Enable the use of stack probes for supported calling conventions.",
        offset: 8,
        detail: detail::Detail::Bool { bit: 0 },
    },
    detail::Descriptor {
        name: "probestack_func_adjusts_sp",
        description: "Enable if the stack probe adjusts the stack pointer.",
        offset: 8,
        detail: detail::Detail::Bool { bit: 1 },
    },
    detail::Descriptor {
        name: "enable_jump_tables",
        description: "Enable the use of jump tables in generated machine code.",
        offset: 8,
        detail: detail::Detail::Bool { bit: 2 },
    },
    detail::Descriptor {
        name: "enable_heap_access_spectre_mitigation",
        description: "Enable Spectre mitigation on heap bounds checks.",
        offset: 8,
        detail: detail::Detail::Bool { bit: 3 },
    },
    detail::Descriptor {
        name: "enable_table_access_spectre_mitigation",
        description: "Enable Spectre mitigation on table bounds checks.",
        offset: 8,
        detail: detail::Detail::Bool { bit: 4 },
    },
    detail::Descriptor {
        name: "enable_incremental_compilation_cache_checks",
        description: "Enable additional checks for debugging the incremental compilation cache.",
        offset: 8,
        detail: detail::Detail::Bool { bit: 5 },
    },
];
static ENUMERATORS: [&str; 16] = [
    "none",
    "speed",
    "speed_and_size",
    "none",
    "elf_gd",
    "macho",
    "coff",
    "isa_default",
    "fast",
    "cold",
    "system_v",
    "windows_fastcall",
    "apple_aarch64",
    "probestack",
    "outline",
    "inline",
];
static HASH_TABLE: [u16; 64] = [
    0xffff,
    0xffff,
    0xffff,
    1,
    8,
    0xffff,
    26,
    0xffff,
    10,
    20,
    23,
    0xffff,
    27,
    0xffff,
    17,
    0xffff,
    15,
    0xffff,
    0xffff,
    0xffff,
    0xffff,
    0xffff,
    0xffff,
    0xffff,
    5,
    0xffff,
    0xffff,
    4,
    0,
    11,
    0xffff,
    0xffff,
    0xffff,
    22,
    0xffff,
    14,
    18,
    9,
    16,
    0xffff,
    6,
    0xffff,
    0xffff,
    19,
    3,
    0xffff,
    25,
    0xffff,
    0xffff,
    0xffff,
    21,
    24,
    0xffff,
    7,
    12,
    2,
    0xffff,
    0xffff,
    0xffff,
    0xffff,
    0xffff,
    0xffff,
    13,
    0xffff,
];
static PRESETS: [(u8, u8); 0] = [
];
static TEMPLATE: detail::Template = detail::Template {
    name: "shared",
    descriptors: &DESCRIPTORS,
    enumerators: &ENUMERATORS,
    hash_table: &HASH_TABLE,
    defaults: &[0x00, 0x00, 0x00, 0x0c, 0x00, 0x00, 0x8c, 0x24, 0x1c],
    presets: &PRESETS,
};
/// Create a `settings::Builder` for the shared settings group.
pub fn builder() -> Builder {
    Builder::new(&TEMPLATE)
}
impl fmt::Display for Flags {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "[shared]")?;
        for d in &DESCRIPTORS {
            if !d.detail.is_preset() {
                write!(f, "{} = ", d.name)?;
                TEMPLATE.format_toml_value(d.detail, self.bytes[d.offset as usize], f)?;
                writeln!(f)?;
            }
        }
        Ok(())
    }
}
