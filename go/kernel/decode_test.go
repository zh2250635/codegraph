package kernel

import (
	"bytes"
	"encoding/binary"
	"encoding/json"
	"math"
	"os"
	"strings"
	"testing"
)

type bufferBuilder struct {
	nodes []byte
	edges []byte
	refs  []byte
	arena []byte
}

func newBufferBuilder() *bufferBuilder {
	return &bufferBuilder{}
}

func (b *bufferBuilder) putStr(s string) (uint32, uint32) {
	off := uint32(len(b.arena))
	b.arena = append(b.arena, []byte(s)...)
	return off, uint32(len(s))
}

func (b *bufferBuilder) putOptStr(s *string) (uint32, uint32) {
	if s == nil {
		return NoneSentinel, 0
	}
	return b.putStr(*s)
}

func (b *bufferBuilder) putList(items []string) (uint32, uint32) {
	if items == nil {
		return NoneSentinel, 0
	}
	joined := strings.Join(items, "\x00")
	return b.putStr(joined)
}

func (b *bufferBuilder) pushNode(
	kind uint8,
	visibility uint8,
	flags uint16,
	startLine, endLine, startCol, endCol uint32,
	name, qualifiedName, id string,
	docstring, signature *string,
	decorators, typeParams []string,
	returnType, extraJSON *string,
) {
	buf := make([]byte, NodeRowSize)
	buf[NodeOffsetKind] = kind
	buf[NodeOffsetVisibility] = visibility
	binary.LittleEndian.PutUint16(buf[NodeOffsetFlags:], flags)
	binary.LittleEndian.PutUint32(buf[NodeOffsetStartLine:], startLine)
	binary.LittleEndian.PutUint32(buf[NodeOffsetEndLine:], endLine)
	binary.LittleEndian.PutUint32(buf[NodeOffsetStartColumn:], startCol)
	binary.LittleEndian.PutUint32(buf[NodeOffsetEndColumn:], endCol)

	writeStrRef := func(offset int, s *string) {
		off, length := b.putOptStr(s)
		binary.LittleEndian.PutUint32(buf[offset:], off)
		binary.LittleEndian.PutUint32(buf[offset+4:], length)
	}

	writeStrListRef := func(offset int, list []string) {
		off, length := b.putList(list)
		binary.LittleEndian.PutUint32(buf[offset:], off)
		binary.LittleEndian.PutUint32(buf[offset+4:], length)
	}

	writeStrRef(NodeOffsetName, &name)
	writeStrRef(NodeOffsetQualifiedName, &qualifiedName)
	writeStrRef(NodeOffsetID, &id)
	writeStrRef(NodeOffsetDocstring, docstring)
	writeStrRef(NodeOffsetSignature, signature)
	writeStrListRef(NodeOffsetDecorators, decorators)
	writeStrListRef(NodeOffsetTypeParameters, typeParams)
	writeStrRef(NodeOffsetReturnType, returnType)
	writeStrRef(NodeOffsetExtraJSON, extraJSON)
	binary.LittleEndian.PutUint32(buf[NodeOffsetMetrics:], 0)

	b.nodes = append(b.nodes, buf...)
}

func (b *bufferBuilder) pushEdge(
	sourceIdx, targetIdx uint32,
	kind, provenance uint8,
	line, column *uint32,
	metadataJSON *string,
	sourceIDStr, targetIDStr *string,
) {
	buf := make([]byte, EdgeRowSize)
	binary.LittleEndian.PutUint32(buf[EdgeOffsetSourceIdx:], sourceIdx)
	binary.LittleEndian.PutUint32(buf[EdgeOffsetTargetIdx:], targetIdx)
	buf[EdgeOffsetKind] = kind
	buf[EdgeOffsetProvenance] = provenance

	putOptU32 := func(offset int, val *uint32) {
		if val == nil {
			binary.LittleEndian.PutUint32(buf[offset:], NoneSentinel)
		} else {
			binary.LittleEndian.PutUint32(buf[offset:], *val)
		}
	}
	putOptU32(EdgeOffsetLine, line)
	putOptU32(EdgeOffsetColumn, column)

	writeStrRef := func(offset int, s *string) {
		off, length := b.putOptStr(s)
		binary.LittleEndian.PutUint32(buf[offset:], off)
		binary.LittleEndian.PutUint32(buf[offset+4:], length)
	}
	writeStrRef(EdgeOffsetMetadataJSON, metadataJSON)
	writeStrRef(EdgeOffsetSourceIDStr, sourceIDStr)
	writeStrRef(EdgeOffsetTargetIDStr, targetIDStr)

	b.edges = append(b.edges, buf...)
}

func (b *bufferBuilder) pushRef(
	fromIdx uint32,
	kind, flags uint8,
	line, column uint32,
	refName string,
	candidates []string,
	fromIDStr *string,
) {
	buf := make([]byte, RefRowSize)
	binary.LittleEndian.PutUint32(buf[RefOffsetFromIdx:], fromIdx)
	buf[RefOffsetKind] = kind
	buf[RefOffsetFlags] = flags
	binary.LittleEndian.PutUint32(buf[RefOffsetLine:], line)
	binary.LittleEndian.PutUint32(buf[RefOffsetColumn:], column)

	off, length := b.putStr(refName)
	binary.LittleEndian.PutUint32(buf[RefOffsetReferenceName:], off)
	binary.LittleEndian.PutUint32(buf[RefOffsetReferenceName+4:], length)

	cOff, cLen := b.putList(candidates)
	binary.LittleEndian.PutUint32(buf[RefOffsetCandidates:], cOff)
	binary.LittleEndian.PutUint32(buf[RefOffsetCandidates+4:], cLen)

	fOff, fLen := b.putOptStr(fromIDStr)
	binary.LittleEndian.PutUint32(buf[RefOffsetFromIDStr:], fOff)
	binary.LittleEndian.PutUint32(buf[RefOffsetFromIDStr+4:], fLen)

	b.refs = append(b.refs, buf...)
}

func (b *bufferBuilder) build(errorsJSON *string, durationMs float64) Buffers {
	var errOff, errLen uint32 = NoneSentinel, 0
	if errorsJSON != nil {
		errOff, errLen = b.putStr(*errorsJSON)
	}

	meta := make([]byte, MetaSize)
	meta[MetaOffsetVersion] = KernelABIVersion
	binary.LittleEndian.PutUint32(meta[MetaOffsetNodeCount:], uint32(len(b.nodes)/NodeRowSize))
	binary.LittleEndian.PutUint32(meta[MetaOffsetEdgeCount:], uint32(len(b.edges)/EdgeRowSize))
	binary.LittleEndian.PutUint32(meta[MetaOffsetRefCount:], uint32(len(b.refs)/RefRowSize))
	binary.LittleEndian.PutUint32(meta[MetaOffsetArenaLen:], uint32(len(b.arena)))
	binary.LittleEndian.PutUint32(meta[MetaOffsetErrorsOff:], errOff)
	binary.LittleEndian.PutUint32(meta[MetaOffsetErrorsLen:], errLen)
	binary.LittleEndian.PutUint64(meta[MetaOffsetDurationMs:], math.Float64bits(durationMs))

	return Buffers{
		Meta:  meta,
		Nodes: b.nodes,
		Edges: b.edges,
		Refs:  b.refs,
		Arena: b.arena,
	}
}

func TestValidateContract(t *testing.T) {
	valid := &Contract{
		ABIVersion:    uint32(KernelABIVersion),
		KernelVersion: "1.6.0",
		NodeKinds:     NodeKinds,
		EdgeKinds:     EdgeKinds,
		Languages:     Languages,
	}
	if err := ValidateContract(valid); err != nil {
		t.Fatalf("expected valid contract, got %v", err)
	}

	if err := ValidateContract(nil); err == nil {
		t.Fatal("expected error on nil contract")
	}

	badABI := *valid
	badABI.ABIVersion = 99
	if err := ValidateContract(&badABI); err == nil {
		t.Fatal("expected error on bad ABI version")
	}

	badNodes := *valid
	badNodes.NodeKinds = []string{"file", "wrong"}
	if err := ValidateContract(&badNodes); err == nil {
		t.Fatal("expected error on mismatched NodeKinds")
	}

	badEdges := *valid
	badEdges.EdgeKinds = []string{"contains"}
	if err := ValidateContract(&badEdges); err == nil {
		t.Fatal("expected error on mismatched EdgeKinds")
	}
}

func TestSupportedLanguage(t *testing.T) {
	for _, lang := range []string{"typescript", "go", "python", "rust", "c", "cpp", "java"} {
		if !SupportedLanguage(lang) {
			t.Errorf("expected %s to be supported", lang)
		}
	}
	if SupportedLanguage("unsupported_language_xyz") {
		t.Error("expected unsupported language to return false")
	}
}

func buildFullExtractionBuffers() (Buffers, int64) {
	b := newBufferBuilder()

	// 1. Push a fully populated Node
	var flags uint16
	flags |= 1 << (FlagIsExported * 2)   // present
	flags |= 1 << (FlagIsExported*2 + 1) // value = true
	flags |= 1 << (FlagIsAsync * 2)      // present, value = false

	doc := "Function documentation"
	sig := "func Calculate(x int) int"
	ret := "int"
	extra := `{"escapeHatch": "customValue", "isAbstract": true}`
	b.pushNode(
		7, // function
		1, // public
		flags,
		10, 20, 0, 15,
		"Calculate", "pkg::Calculate", "func:hash123",
		&doc, &sig,
		[]string{"@route", "@auth"}, []string{"T", "U"},
		&ret, &extra,
	)

	// 2. Push Edges
	line0 := uint32(0)
	col0 := uint32(0)
	metaJSON := `{"weight": 2.5, "tag": "callsite"}`
	tgtStr := "external::target"
	b.pushEdge(
		0, NoneSentinel,
		1, // calls
		1, // tree-sitter
		&line0, &col0,
		&metaJSON,
		nil, &tgtStr,
	)

	srcStr := "external::caller"
	b.pushEdge(
		NoneSentinel, 0,
		0, // contains
		0, // absent provenance
		nil, nil,
		nil,
		&srcStr, nil,
	)

	// 3. Push Refs
	b.pushRef(
		0,
		FunctionRefCode,
		RefFlagFilePath,
		15, 4,
		"callbackTarget",
		[]string{"candidate1", "candidate2"},
		nil,
	)

	fromID := "external::from"
	b.pushRef(
		NoneSentinel,
		1, // calls
		0,
		18, 2,
		"standardTarget",
		nil,
		&fromID,
	)

	// 4. Errors
	errsJSON := `[{"message":"warning msg","severity":"warning","line":5,"column":0},{"message":"err msg","severity":"error"}]`
	buffers := b.build(&errsJSON, 42.5)
	fixedUpdatedAt := int64(1700000000123)
	return buffers, fixedUpdatedAt
}

func verifyFullExtractionNode(t *testing.T, n Node, fixedUpdatedAt int64) {
	if n.ID != "func:hash123" || n.Name != "Calculate" || n.QualifiedName != "pkg::Calculate" {
		t.Errorf("node identity mismatch: %+v", n)
	}
	if n.FilePath != "src/math.go" || n.Language != "go" || n.UpdatedAt != fixedUpdatedAt {
		t.Errorf("node context mismatch: %+v", n)
	}
	if n.Visibility == nil || *n.Visibility != "public" {
		t.Errorf("expected public visibility, got %v", n.Visibility)
	}
	if n.IsExported == nil || !*n.IsExported {
		t.Errorf("expected isExported=true, got %v", n.IsExported)
	}
	if n.IsAsync == nil || *n.IsAsync {
		t.Errorf("expected isAsync=false, got %v", n.IsAsync)
	}
	// Overridden by extraJson: isAbstract=true
	if n.IsAbstract == nil || !*n.IsAbstract {
		t.Errorf("expected isAbstract overridden to true, got %v", n.IsAbstract)
	}
	if len(n.Decorators) != 2 || n.Decorators[0] != "@route" {
		t.Errorf("decorators mismatch: %v", n.Decorators)
	}
	if len(n.TypeParameters) != 2 || n.TypeParameters[1] != "U" {
		t.Errorf("typeParameters mismatch: %v", n.TypeParameters)
	}

	data, err := json.Marshal(&n)
	if err != nil {
		t.Fatalf("json.Marshal(node) failed: %v", err)
	}
	var nodeMap map[string]any
	if err := json.Unmarshal(data, &nodeMap); err != nil {
		t.Fatalf("unmarshal node JSON failed: %v", err)
	}
	if nodeMap["escapeHatch"] != "customValue" {
		t.Errorf("expected extraJson escapeHatch in JSON, got %v", nodeMap["escapeHatch"])
	}
}

func verifyFullExtractionEdges(t *testing.T, edges []Edge) {
	if len(edges) != 2 {
		t.Fatalf("expected 2 edges, got %d", len(edges))
	}
	e0 := edges[0]
	if e0.Source != "func:hash123" || e0.Target != "external::target" || e0.Kind != "calls" {
		t.Errorf("edge 0 mismatch: %+v", e0)
	}
	if e0.Provenance == nil || *e0.Provenance != "tree-sitter" {
		t.Errorf("edge 0 provenance mismatch: %v", e0.Provenance)
	}
	if e0.Line == nil || *e0.Line != 0 || e0.Column == nil || *e0.Column != 0 {
		t.Errorf("edge 0 line/col 0 should be preserved, got line=%v, col=%v", e0.Line, e0.Column)
	}

	edge0JSON, _ := json.Marshal(&e0)
	if !bytes.Contains(edge0JSON, []byte(`"line":0`)) {
		t.Errorf("expected \"line\":0 in edge JSON, got %s", string(edge0JSON))
	}
}

func verifyFullExtractionRefs(t *testing.T, refs []UnresolvedReference) {
	if len(refs) != 2 {
		t.Fatalf("expected 2 refs, got %d", len(refs))
	}
	r0 := refs[0]
	if r0.ReferenceKind != "function_ref" {
		t.Errorf("expected function_ref kind, got %s", r0.ReferenceKind)
	}
	if r0.FilePath == nil || *r0.FilePath != "src/math.go" {
		t.Errorf("expected ref filePath to be src/math.go, got %v", r0.FilePath)
	}
	if len(r0.Candidates) != 2 || r0.Candidates[0] != "candidate1" {
		t.Errorf("candidates mismatch: %v", r0.Candidates)
	}

	r1 := refs[1]
	if r1.FilePath != nil {
		t.Errorf("expected ref 1 filePath to be absent, got %v", r1.FilePath)
	}
	if r1.Candidates != nil {
		t.Errorf("expected ref 1 candidates to be nil, got %v", r1.Candidates)
	}
}

func verifyFullExtractionErrorsAndResult(t *testing.T, res *ExtractionResult) {
	if len(res.Errors) != 2 {
		t.Fatalf("expected 2 errors, got %d", len(res.Errors))
	}
	if res.Errors[0].Line == nil || *res.Errors[0].Line != 5 {
		t.Errorf("error 0 line mismatch: %v", res.Errors[0].Line)
	}

	resBytes, err := json.Marshal(res)
	if err != nil {
		t.Fatalf("json.Marshal(res) failed: %v", err)
	}
	var resMap map[string]any
	if err := json.Unmarshal(resBytes, &resMap); err != nil {
		t.Fatalf("unmarshal res JSON failed: %v", err)
	}
	if resMap["durationMs"] != 42.5 {
		t.Errorf("expected durationMs 42.5 in JSON, got %v", resMap["durationMs"])
	}
}

func TestFullExtractionDecode(t *testing.T) {
	buffers, fixedUpdatedAt := buildFullExtractionBuffers()

	res, err := DecodeAt(buffers, "src/math.go", "go", fixedUpdatedAt)
	if err != nil {
		t.Fatalf("unexpected Decode error: %v", err)
	}
	if res.DurationMs != 42.5 {
		t.Errorf("expected DurationMs 42.5, got %v", res.DurationMs)
	}
	if len(res.Nodes) != 1 {
		t.Fatalf("expected 1 node, got %d", len(res.Nodes))
	}

	verifyFullExtractionNode(t, res.Nodes[0], fixedUpdatedAt)
	verifyFullExtractionEdges(t, res.Edges)
	verifyFullExtractionRefs(t, res.UnresolvedReferences)
	verifyFullExtractionErrorsAndResult(t, res)
}
func TestEmptyTables(t *testing.T) {
	b := newBufferBuilder()
	buffers := b.build(nil, 0)

	res, err := Decode(buffers, "test.ts", "typescript")
	if err != nil {
		t.Fatalf("Decode empty tables failed: %v", err)
	}

	if len(res.Nodes) != 0 || len(res.Edges) != 0 || len(res.UnresolvedReferences) != 0 || len(res.Errors) != 0 {
		t.Errorf("expected all empty slices, got %+v", res)
	}

	out, err := json.Marshal(res)
	if err != nil {
		t.Fatalf("marshal failed: %v", err)
	}
	expected := `{"nodes":[],"edges":[],"unresolvedReferences":[],"errors":[],"durationMs":0}`
	if string(out) != expected {
		t.Errorf("expected %s, got %s", expected, string(out))
	}
}

func TestValidationErrors(t *testing.T) {
	// Meta too short
	if _, err := Decode(Buffers{Meta: make([]byte, 10)}, "f", "go"); err == nil {
		t.Error("expected error for short meta")
	}

	// Bad ABI version
	badMeta := make([]byte, MetaSize)
	badMeta[MetaOffsetVersion] = 9
	if _, err := Decode(Buffers{Meta: badMeta}, "f", "go"); err == nil {
		t.Error("expected error for bad ABI version")
	}

	// Table count exceeds safe limit
	oomMeta := make([]byte, MetaSize)
	oomMeta[MetaOffsetVersion] = KernelABIVersion
	binary.LittleEndian.PutUint32(oomMeta[MetaOffsetNodeCount:], MaxTableEntries+1)
	if _, err := Decode(Buffers{Meta: oomMeta}, "f", "go"); err == nil {
		t.Error("expected error for exceeding MaxTableEntries")
	}

	// Buffer shorter than count * rowSize
	shortNodeMeta := make([]byte, MetaSize)
	shortNodeMeta[MetaOffsetVersion] = KernelABIVersion
	binary.LittleEndian.PutUint32(shortNodeMeta[MetaOffsetNodeCount:], 2)
	if _, err := Decode(Buffers{Meta: shortNodeMeta, Nodes: make([]byte, NodeRowSize)}, "f", "go"); err == nil {
		t.Error("expected error for truncated nodes buffer")
	}

	// Node row index out of bounds in edge
	b := newBufferBuilder()
	b.pushNode(0, 0, 0, 1, 1, 0, 0, "n1", "n1", "id1", nil, nil, nil, nil, nil, nil)
	b.pushEdge(5, NoneSentinel, 0, 0, nil, nil, nil, nil, nil) // sourceIdx=5 > nodeCount=1
	badEdgeBufs := b.build(nil, 0)
	if _, err := Decode(badEdgeBufs, "f", "go"); err == nil {
		t.Error("expected error for edge sourceIdx out of bounds")
	}

	// Ref kind out of bounds
	b2 := newBufferBuilder()
	b2.pushNode(0, 0, 0, 1, 1, 0, 0, "n1", "n1", "id1", nil, nil, nil, nil, nil, nil)
	b2.pushRef(0, 100, 0, 1, 1, "ref", nil, nil) // kind=100 (not 200 and >= 12)
	badRefBufs := b2.build(nil, 0)
	if _, err := Decode(badRefBufs, "f", "go"); err == nil {
		t.Error("expected error for invalid ref kind")
	}

	// Arena string bounds overflow
	b3 := newBufferBuilder()
	buf := make([]byte, NodeRowSize)
	binary.LittleEndian.PutUint32(buf[NodeOffsetID:], 1000) // arena off=1000
	binary.LittleEndian.PutUint32(buf[NodeOffsetID+4:], 50) // len=50
	b3.nodes = append(b3.nodes, buf...)
	badArenaBufs := b3.build(nil, 0)
	if _, err := Decode(badArenaBufs, "f", "go"); err == nil {
		t.Error("expected error for arena string out of bounds")
	}

	// Invalid UTF-8 in arena
	b4 := newBufferBuilder()
	invalidUTF8Off := uint32(len(b4.arena))
	b4.arena = append(b4.arena, 0xff, 0xfe) // invalid utf8
	nodeBuf := make([]byte, NodeRowSize)
	binary.LittleEndian.PutUint32(nodeBuf[NodeOffsetID:], invalidUTF8Off)
	binary.LittleEndian.PutUint32(nodeBuf[NodeOffsetID+4:], 2)
	b4.nodes = append(b4.nodes, nodeBuf...)
	badUTF8Bufs := b4.build(nil, 0)
	if _, err := Decode(badUTF8Bufs, "f", "go"); err == nil {
		t.Error("expected error for invalid utf-8 string")
	}
}

func FuzzDecode(f *testing.F) {
	// Seed with valid empty buffer
	b := newBufferBuilder()
	bufs := b.build(nil, 0)
	f.Add(bufs.Meta, bufs.Nodes, bufs.Edges, bufs.Refs, bufs.Arena)

	// Empty-only seeds rarely reach row decoding. Real release tables exercise
	// strings, endpoint indices, extra JSON and all optional fields under mutation.
	data, err := os.ReadFile("testdata/napi-v1.6.0.json")
	if err != nil {
		f.Fatal(err)
	}
	var oracle napiOracle
	if err := json.Unmarshal(data, &oracle); err != nil {
		f.Fatal(err)
	}
	seeded := make(map[string]bool)
	for _, entry := range oracle.Cases {
		if entry.Error != "" || seeded[entry.Language] {
			continue
		}
		seeded[entry.Language] = true
		b := entry.Buffers
		f.Add(b.Meta, b.Nodes, b.Edges, b.Refs, b.Arena)
	}

	f.Fuzz(func(t *testing.T, meta, nodes, edges, refs, arena []byte) {
		buffers := Buffers{
			Meta:  meta,
			Nodes: nodes,
			Edges: edges,
			Refs:  refs,
			Arena: arena,
		}
		// Arbitrary bytes must never panic. Successful results must also remain
		// JSON-serializable, including finite timings and extra JSON properties.
		result, err := Decode(buffers, "fuzz.ts", "typescript")
		if err != nil {
			return
		}
		if _, err := json.Marshal(result); err != nil {
			t.Fatalf("successful decode produced invalid JSON: %v", err)
		}
	})
}

func TestNodeJSONTriStateAndEmptyLists(t *testing.T) {
	node := Node{
		ID:             "n1",
		Kind:           "function",
		Name:           "testFn",
		QualifiedName:  "pkg::testFn",
		FilePath:       "file.go",
		Language:       "go",
		StartLine:      1,
		EndLine:        2,
		StartColumn:    0,
		EndColumn:      10,
		UpdatedAt:      1000,
		IsExported:     BoolPtr(false),
		IsAsync:        nil,        // absent
		Decorators:     []string{}, // empty list
		TypeParameters: nil,        // absent list
	}

	data, err := json.Marshal(&node)
	if err != nil {
		t.Fatalf("marshal node failed: %v", err)
	}
	s := string(data)
	if !strings.Contains(s, `"isExported":false`) {
		t.Errorf("expected isExported:false, got %s", s)
	}
	if strings.Contains(s, "isAsync") {
		t.Errorf("expected isAsync to be omitted, got %s", s)
	}
	if !strings.Contains(s, `"decorators":[]`) {
		t.Errorf("expected decorators:[], got %s", s)
	}
	if strings.Contains(s, "typeParameters") {
		t.Errorf("expected typeParameters to be omitted, got %s", s)
	}

	// Unmarshal back
	var node2 Node
	if err := json.Unmarshal(data, &node2); err != nil {
		t.Fatalf("unmarshal node failed: %v", err)
	}
	if node2.IsExported == nil || *node2.IsExported != false {
		t.Errorf("expected node2 isExported to be false, got %v", node2.IsExported)
	}
	if node2.IsAsync != nil {
		t.Errorf("expected node2 isAsync to be nil, got %v", node2.IsAsync)
	}
	if node2.Decorators == nil || len(node2.Decorators) != 0 {
		t.Errorf("expected node2 decorators to be non-nil empty slice, got %v", node2.Decorators)
	}
}

func TestEdgeJSONZeroAndEmptyMetadata(t *testing.T) {
	edge := Edge{
		Source:   "s",
		Target:   "t",
		Kind:     "calls",
		Line:     IntPtr(0),
		Column:   IntPtr(0),
		Metadata: make(map[string]any), // empty metadata
	}

	data, err := json.Marshal(&edge)
	if err != nil {
		t.Fatalf("marshal edge failed: %v", err)
	}
	s := string(data)
	if !strings.Contains(s, `"line":0`) || !strings.Contains(s, `"column":0`) {
		t.Errorf("expected line:0 and column:0 preserved, got %s", s)
	}
	if !strings.Contains(s, `"metadata":{}`) {
		t.Errorf("expected metadata:{}, got %s", s)
	}
}

func TestMalformedJSONErrors(t *testing.T) {
	// 1. ExtraJSON malformed
	b1 := newBufferBuilder()
	badExtra := "{invalid_json"
	b1.pushNode(0, 0, 0, 1, 1, 0, 0, "n", "n", "id", nil, nil, nil, nil, nil, &badExtra)
	bufs1 := b1.build(nil, 0)
	if _, err := Decode(bufs1, "f", "go"); err == nil {
		t.Error("expected error for malformed extraJSON")
	}

	// 2. Edge MetadataJSON malformed
	b2 := newBufferBuilder()
	b2.pushNode(0, 0, 0, 1, 1, 0, 0, "n", "n", "id", nil, nil, nil, nil, nil, nil)
	badMeta := "[not_an_object"
	b2.pushEdge(0, 0, 0, 0, nil, nil, &badMeta, nil, nil)
	bufs2 := b2.build(nil, 0)
	if _, err := Decode(bufs2, "f", "go"); err == nil {
		t.Error("expected error for malformed edge metadata JSON")
	}

	// 3. ErrorsJSON malformed
	b3 := newBufferBuilder()
	badErrJSON := "not a json list"
	bufs3 := b3.build(&badErrJSON, 0)
	if _, err := Decode(bufs3, "f", "go"); err == nil {
		t.Error("expected error for malformed errors JSON")
	}
}

func TestUnicodeHandling(t *testing.T) {
	b := newBufferBuilder()
	doc := "说明文档 🚀"
	extra := `{"作者": "张三", "emoji": "🎉"}`
	b.pushNode(0, 0, 0, 1, 1, 0, 0, "测试函数", "包::测试函数", "func:中文ID", &doc, nil, nil, nil, nil, &extra)
	bufs := b.build(nil, 1.0)

	res, err := Decode(bufs, "main.go", "go")
	if err != nil {
		t.Fatalf("decode unicode failed: %v", err)
	}
	n := res.Nodes[0]
	if n.Name != "测试函数" || n.ID != "func:中文ID" || *n.Docstring != "说明文档 🚀" {
		t.Errorf("unicode mismatch: %+v", n)
	}
	if string(n.Extra["作者"]) != `"张三"` {
		t.Errorf("unicode extra mismatch: %s", string(n.Extra["作者"]))
	}
}

func TestApplyKnownNodeProperties(t *testing.T) {
	node := Node{ID: "n0"}
	extra := `{"name":"newName","qualifiedName":"newQName","filePath":"newPath.go","language":"go","startLine":10,"endLine":20,"startColumn":1,"endColumn":5,"docstring":"newDoc","signature":"newSig","visibility":"private","isAsync":true,"isStatic":true,"decorators":["@d1"],"typeParameters":["T"],"returnType":"void","updatedAt":123456}`
	if err := applyExtraJSON(&node, extra); err != nil {
		t.Fatalf("applyExtraJSON failed: %v", err)
	}

	if node.Name != "newName" || node.QualifiedName != "newQName" || node.FilePath != "newPath.go" {
		t.Errorf("basic strings not updated: %+v", node)
	}
	if node.StartLine != 10 || node.EndLine != 20 || node.StartColumn != 1 || node.EndColumn != 5 {
		t.Errorf("lines/cols not updated: %+v", node)
	}
	if node.Docstring == nil || *node.Docstring != "newDoc" {
		t.Errorf("docstring not updated: %v", node.Docstring)
	}
	if node.Signature == nil || *node.Signature != "newSig" {
		t.Errorf("signature not updated: %v", node.Signature)
	}
	if node.Visibility == nil || *node.Visibility != "private" {
		t.Errorf("visibility not updated: %v", node.Visibility)
	}
	if node.IsAsync == nil || !*node.IsAsync || node.IsStatic == nil || !*node.IsStatic {
		t.Errorf("flags not updated: async=%v, static=%v", node.IsAsync, node.IsStatic)
	}
	if len(node.Decorators) != 1 || node.Decorators[0] != "@d1" {
		t.Errorf("decorators not updated: %v", node.Decorators)
	}
	if len(node.TypeParameters) != 1 || node.TypeParameters[0] != "T" {
		t.Errorf("typeParameters not updated: %v", node.TypeParameters)
	}
	if node.ReturnType == nil || *node.ReturnType != "void" {
		t.Errorf("returnType not updated: %v", node.ReturnType)
	}
	if node.UpdatedAt != 123456 {
		t.Errorf("updatedAt not updated: %v", node.UpdatedAt)
	}

	_ = Int64Ptr(42)
	_ = StringPtr("str")
}
