package kernel

// KernelABIVersion specifies the buffer ABI version across native boundary.
const KernelABIVersion uint8 = 2

// NoneSentinel indicates an absent slot in u32 offsets and indices.
const NoneSentinel uint32 = 0xffffffff

// Buffer row sizes in bytes.
const (
	MetaSize    = 36
	NodeRowSize = 96
	EdgeRowSize = 44
	RefRowSize  = 40
)

// Meta byte offsets.
const (
	MetaOffsetVersion    = 0  // u8
	MetaOffsetNodeCount  = 4  // u32
	MetaOffsetEdgeCount  = 8  // u32
	MetaOffsetRefCount   = 12 // u32
	MetaOffsetArenaLen   = 16 // u32
	MetaOffsetErrorsOff  = 20 // u32
	MetaOffsetErrorsLen  = 24 // u32
	MetaOffsetDurationMs = 28 // f64 (8 bytes)
)

// Node row byte offsets.
const (
	NodeOffsetKind           = 0  // u8
	NodeOffsetVisibility     = 1  // u8
	NodeOffsetFlags          = 2  // u16
	NodeOffsetStartLine      = 4  // u32
	NodeOffsetEndLine        = 8  // u32
	NodeOffsetStartColumn    = 12 // u32
	NodeOffsetEndColumn      = 16 // u32
	NodeOffsetName           = 20 // str (off: u32, len: u32)
	NodeOffsetQualifiedName  = 28 // str
	NodeOffsetID             = 36 // str
	NodeOffsetDocstring      = 44 // str
	NodeOffsetSignature      = 52 // str
	NodeOffsetDecorators     = 60 // str (NUL-joined list)
	NodeOffsetTypeParameters = 68 // str (NUL-joined list)
	NodeOffsetReturnType     = 76 // str
	NodeOffsetExtraJSON      = 84 // str (JSON escape hatch)
	NodeOffsetMetrics        = 92 // u32 (reserved)
)

// Edge row byte offsets.
const (
	EdgeOffsetSourceIdx    = 0  // u32
	EdgeOffsetTargetIdx    = 4  // u32
	EdgeOffsetKind         = 8  // u8
	EdgeOffsetProvenance   = 9  // u8
	EdgeOffsetLine         = 12 // u32
	EdgeOffsetColumn       = 16 // u32
	EdgeOffsetMetadataJSON = 20 // str
	EdgeOffsetSourceIDStr  = 28 // str
	EdgeOffsetTargetIDStr  = 36 // str
)

// Ref row byte offsets.
const (
	RefOffsetFromIdx       = 0  // u32
	RefOffsetKind          = 4  // u8
	RefOffsetFlags         = 5  // u8
	RefOffsetLine          = 8  // u32
	RefOffsetColumn        = 12 // u32
	RefOffsetReferenceName = 16 // str
	RefOffsetCandidates    = 24 // str (NUL-joined list)
	RefOffsetFromIDStr     = 32 // str
)

// FunctionRefCode is the ReferenceKind wire code for internal-only function_ref (#756).
const FunctionRefCode uint8 = 200

// RefFlagFilePath indicates the ref carries the extracted file's path.
const RefFlagFilePath uint8 = 1

// Node bool-flag bit pair offsets (bit 2n = present, bit 2n+1 = value).
const (
	FlagIsExported = 0
	FlagIsAsync    = 1
	FlagIsStatic   = 2
	FlagIsAbstract = 3
)

// NodeKinds mirrors src/types.ts NODE_KINDS; order is part of wire contract.
var NodeKinds = []string{
	"file",
	"module",
	"class",
	"struct",
	"interface",
	"trait",
	"protocol",
	"function",
	"method",
	"property",
	"field",
	"variable",
	"constant",
	"enum",
	"enum_member",
	"type_alias",
	"namespace",
	"parameter",
	"import",
	"export",
	"route",
	"component",
	"union",
}

// EdgeKinds mirrors src/types.ts EDGE_KINDS; order is part of wire contract.
var EdgeKinds = []string{
	"contains",
	"calls",
	"imports",
	"exports",
	"extends",
	"implements",
	"references",
	"type_of",
	"returns",
	"instantiates",
	"overrides",
	"decorates",
}

// Languages mirrors src/types.ts LANGUAGES.
var Languages = []string{
	"typescript",
	"javascript",
	"tsx",
	"jsx",
	"arkts",
	"python",
	"go",
	"rust",
	"java",
	"c",
	"cpp",
	"csharp",
	"razor",
	"php",
	"ruby",
	"swift",
	"kotlin",
	"dart",
	"svelte",
	"vue",
	"astro",
	"liquid",
	"pascal",
	"scala",
	"lua",
	"luau",
	"objc",
	"r",
	"solidity",
	"nix",
	"yaml",
	"twig",
	"xml",
	"properties",
	"cfml",
	"cfscript",
	"cfquery",
	"cobol",
	"vbnet",
	"erlang",
	"terraform",
	"unknown",
}

// Visibilities defines visibility values indexed by u8 (0 is absent).
var Visibilities = []string{
	"", // 0 = absent
	"public",
	"private",
	"protected",
	"internal",
}

// Provenances defines edge provenance values indexed by u8 (0 is absent).
var Provenances = []string{
	"", // 0 = absent
	"tree-sitter",
	"scip",
	"heuristic",
}
