package kernel

import (
	"encoding/binary"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"strings"
	"time"
	"unicode/utf8"
)

// MaxTableEntries defines an upper bound on count fields to prevent blind OOM allocation.
const MaxTableEntries uint32 = 5_000_000

type metaHeader struct {
	nodeCount  uint32
	edgeCount  uint32
	refCount   uint32
	arenaLen   uint32
	errorsOff  uint32
	errorsLen  uint32
	durationMs float64
}

// Decode extracts kernel flat buffers into an ExtractionResult using current UnixMilli timestamp.
func Decode(buffers Buffers, filePath, language string) (*ExtractionResult, error) {
	return DecodeAt(buffers, filePath, language, time.Now().UnixMilli())
}

// DecodeAt extracts kernel flat buffers with a caller-specified updatedAt timestamp for differential reproducibility.
func DecodeAt(buffers Buffers, filePath, language string, updatedAt int64) (*ExtractionResult, error) {
	header, err := validateAndParseMeta(buffers)
	if err != nil {
		return nil, err
	}
	if err := validateBufferLengths(buffers, header); err != nil {
		return nil, err
	}

	nodes, idByRow, err := decodeNodes(buffers, header.nodeCount, filePath, language, updatedAt)
	if err != nil {
		return nil, err
	}

	edges, err := decodeEdges(buffers, header.edgeCount, header.nodeCount, idByRow)
	if err != nil {
		return nil, err
	}

	refs, err := decodeRefs(buffers, header.refCount, header.nodeCount, idByRow, filePath)
	if err != nil {
		return nil, err
	}

	extErrors, err := decodeErrors(buffers.Arena, header.errorsOff, header.errorsLen)
	if err != nil {
		return nil, err
	}

	return &ExtractionResult{
		Nodes:                nodes,
		Edges:                edges,
		UnresolvedReferences: refs,
		Errors:               extErrors,
		DurationMs:           header.durationMs,
	}, nil
}

func validateAndParseMeta(buffers Buffers) (metaHeader, error) {
	if len(buffers.Meta) < MetaSize {
		return metaHeader{}, fmt.Errorf("kernel meta too short: %d < %d", len(buffers.Meta), MetaSize)
	}
	version := buffers.Meta[MetaOffsetVersion]
	if version != KernelABIVersion {
		return metaHeader{}, fmt.Errorf("kernel buffer ABI %d != expected %d", version, KernelABIVersion)
	}

	nodeCount := binary.LittleEndian.Uint32(buffers.Meta[MetaOffsetNodeCount:])
	edgeCount := binary.LittleEndian.Uint32(buffers.Meta[MetaOffsetEdgeCount:])
	refCount := binary.LittleEndian.Uint32(buffers.Meta[MetaOffsetRefCount:])
	arenaLen := binary.LittleEndian.Uint32(buffers.Meta[MetaOffsetArenaLen:])
	errorsOff := binary.LittleEndian.Uint32(buffers.Meta[MetaOffsetErrorsOff:])
	errorsLen := binary.LittleEndian.Uint32(buffers.Meta[MetaOffsetErrorsLen:])
	durationBits := binary.LittleEndian.Uint64(buffers.Meta[MetaOffsetDurationMs:])
	durationMs := math.Float64frombits(durationBits)
	if math.IsNaN(durationMs) || math.IsInf(durationMs, 0) || durationMs < 0 {
		return metaHeader{}, errors.New("invalid kernel durationMs")
	}

	if nodeCount > MaxTableEntries || edgeCount > MaxTableEntries || refCount > MaxTableEntries {
		return metaHeader{}, fmt.Errorf("table count exceeds safety limit %d (nodes:%d, edges:%d, refs:%d)",
			MaxTableEntries, nodeCount, edgeCount, refCount)
	}

	return metaHeader{
		nodeCount:  nodeCount,
		edgeCount:  edgeCount,
		refCount:   refCount,
		arenaLen:   arenaLen,
		errorsOff:  errorsOff,
		errorsLen:  errorsLen,
		durationMs: durationMs,
	}, nil
}

func validateBufferLengths(buffers Buffers, h metaHeader) error {
	reqNodes := uint64(h.nodeCount) * uint64(NodeRowSize)
	if uint64(len(buffers.Nodes)) != reqNodes {
		return fmt.Errorf("nodes buffer size mismatch: got %d, expected %d", len(buffers.Nodes), reqNodes)
	}

	reqEdges := uint64(h.edgeCount) * uint64(EdgeRowSize)
	if uint64(len(buffers.Edges)) != reqEdges {
		return fmt.Errorf("edges buffer size mismatch: got %d, expected %d", len(buffers.Edges), reqEdges)
	}

	reqRefs := uint64(h.refCount) * uint64(RefRowSize)
	if uint64(len(buffers.Refs)) != reqRefs {
		return fmt.Errorf("refs buffer size mismatch: got %d, expected %d", len(buffers.Refs), reqRefs)
	}

	if uint64(len(buffers.Arena)) != uint64(h.arenaLen) {
		return fmt.Errorf("arena buffer size mismatch: got %d, expected %d", len(buffers.Arena), h.arenaLen)
	}
	return nil
}

func readStr(arena, row []byte, offset int) (*string, error) {
	off := binary.LittleEndian.Uint32(row[offset:])
	if off == NoneSentinel {
		return nil, nil
	}
	length := binary.LittleEndian.Uint32(row[offset+4:])
	if uint64(off)+uint64(length) > uint64(len(arena)) {
		return nil, fmt.Errorf("arena string out of bounds: off=%d, len=%d, arenaLen=%d", off, length, len(arena))
	}
	slice := arena[int(off):int(uint64(off)+uint64(length))]
	if !utf8.Valid(slice) {
		return nil, errors.New("invalid utf-8 in arena string")
	}
	s := string(slice)
	return &s, nil
}

func readStrList(arena, row []byte, offset int) ([]string, error) {
	s, err := readStr(arena, row, offset)
	if err != nil {
		return nil, err
	}
	if s == nil {
		return nil, nil
	}
	return strings.Split(*s, "\x00"), nil
}

func flagVal(flags uint16, pair uint16) *bool {
	if (flags & (1 << (pair * 2))) == 0 {
		return nil
	}
	val := (flags & (1 << (pair*2 + 1))) != 0
	return &val
}

func u32opt(row []byte, offset int) *int {
	v := binary.LittleEndian.Uint32(row[offset:])
	if v == NoneSentinel {
		return nil
	}
	val := int(v)
	return &val
}

func decodeNodes(buffers Buffers, nodeCount uint32, filePath, language string, updatedAt int64) ([]Node, []string, error) {
	nodes := make([]Node, nodeCount)
	idByRow := make([]string, nodeCount)

	for i := 0; i < int(nodeCount); i++ {
		row := buffers.Nodes[i*NodeRowSize : (i+1)*NodeRowSize]
		node, idStr, err := decodeNodeRow(buffers.Arena, row, filePath, language, updatedAt)
		if err != nil {
			return nil, nil, fmt.Errorf("node row %d error: %w", i, err)
		}
		idByRow[i] = idStr
		nodes[i] = node
	}
	return nodes, idByRow, nil
}

func decodeNodeRow(arena, row []byte, filePath, language string, updatedAt int64) (Node, string, error) {
	kindByte := row[NodeOffsetKind]
	if int(kindByte) >= len(NodeKinds) {
		return Node{}, "", fmt.Errorf("unknown NodeKind index: %d", kindByte)
	}

	visByte := row[NodeOffsetVisibility]
	if int(visByte) >= len(Visibilities) {
		return Node{}, "", fmt.Errorf("unknown visibility index: %d", visByte)
	}

	idPtr, err := readStr(arena, row, NodeOffsetID)
	if err != nil || idPtr == nil {
		return Node{}, "", errors.New("node missing id")
	}

	namePtr, err := readStr(arena, row, NodeOffsetName)
	if err != nil || namePtr == nil {
		return Node{}, "", errors.New("node missing name")
	}

	qNamePtr, err := readStr(arena, row, NodeOffsetQualifiedName)
	if err != nil || qNamePtr == nil {
		return Node{}, "", errors.New("node missing qualifiedName")
	}

	flags := binary.LittleEndian.Uint16(row[NodeOffsetFlags:])
	node := Node{
		ID:            *idPtr,
		Kind:          NodeKinds[kindByte],
		Name:          *namePtr,
		QualifiedName: *qNamePtr,
		FilePath:      filePath,
		Language:      language,
		StartLine:     int(binary.LittleEndian.Uint32(row[NodeOffsetStartLine:])),
		EndLine:       int(binary.LittleEndian.Uint32(row[NodeOffsetEndLine:])),
		StartColumn:   int(binary.LittleEndian.Uint32(row[NodeOffsetStartColumn:])),
		EndColumn:     int(binary.LittleEndian.Uint32(row[NodeOffsetEndColumn:])),
		UpdatedAt:     updatedAt,
		IsExported:    flagVal(flags, FlagIsExported),
		IsAsync:       flagVal(flags, FlagIsAsync),
		IsStatic:      flagVal(flags, FlagIsStatic),
		IsAbstract:    flagVal(flags, FlagIsAbstract),
	}

	if visByte != 0 {
		node.Visibility = &Visibilities[visByte]
	}
	if err := decodeNodeOptionals(arena, row, &node); err != nil {
		return Node{}, "", err
	}
	return node, *idPtr, nil
}

func decodeNodeOptionals(arena, row []byte, node *Node) error {
	var err error
	if node.Docstring, err = readStr(arena, row, NodeOffsetDocstring); err != nil {
		return err
	}
	if node.Signature, err = readStr(arena, row, NodeOffsetSignature); err != nil {
		return err
	}
	if node.Decorators, err = readStrList(arena, row, NodeOffsetDecorators); err != nil {
		return err
	}
	if node.TypeParameters, err = readStrList(arena, row, NodeOffsetTypeParameters); err != nil {
		return err
	}
	if node.ReturnType, err = readStr(arena, row, NodeOffsetReturnType); err != nil {
		return err
	}

	extraJSON, err := readStr(arena, row, NodeOffsetExtraJSON)
	if err != nil {
		return err
	}
	if extraJSON == nil {
		return nil
	}
	return applyExtraJSON(node, *extraJSON)
}

func applyExtraJSON(node *Node, extraJSON string) error {
	var raw map[string]json.RawMessage
	if err := json.Unmarshal([]byte(extraJSON), &raw); err != nil {
		return fmt.Errorf("extraJson invalid JSON: %w", err)
	}

	if node.Extra == nil {
		node.Extra = make(map[string]json.RawMessage)
	}
	for k, v := range raw {
		node.Extra[k] = v
		if err := applyKnownNodeProperty(node, k, v); err != nil {
			return err
		}
	}
	return nil
}

func applyKnownNodeProperty(node *Node, key string, raw json.RawMessage) error {
	switch key {
	case "id":
		return json.Unmarshal(raw, &node.ID)
	case "kind":
		return json.Unmarshal(raw, &node.Kind)
	case "name":
		return json.Unmarshal(raw, &node.Name)
	case "qualifiedName":
		return json.Unmarshal(raw, &node.QualifiedName)
	case "filePath":
		return json.Unmarshal(raw, &node.FilePath)
	case "language":
		return json.Unmarshal(raw, &node.Language)
	case "startLine":
		return json.Unmarshal(raw, &node.StartLine)
	case "endLine":
		return json.Unmarshal(raw, &node.EndLine)
	case "startColumn":
		return json.Unmarshal(raw, &node.StartColumn)
	case "endColumn":
		return json.Unmarshal(raw, &node.EndColumn)
	case "docstring":
		return json.Unmarshal(raw, &node.Docstring)
	case "signature":
		return json.Unmarshal(raw, &node.Signature)
	case "visibility":
		return json.Unmarshal(raw, &node.Visibility)
	case "isExported":
		return json.Unmarshal(raw, &node.IsExported)
	case "isAsync":
		return json.Unmarshal(raw, &node.IsAsync)
	case "isStatic":
		return json.Unmarshal(raw, &node.IsStatic)
	case "isAbstract":
		return json.Unmarshal(raw, &node.IsAbstract)
	case "decorators":
		return json.Unmarshal(raw, &node.Decorators)
	case "typeParameters":
		return json.Unmarshal(raw, &node.TypeParameters)
	case "returnType":
		return json.Unmarshal(raw, &node.ReturnType)
	case "updatedAt":
		return json.Unmarshal(raw, &node.UpdatedAt)
	}
	return nil
}

func decodeEdges(buffers Buffers, edgeCount, nodeCount uint32, idByRow []string) ([]Edge, error) {
	edges := make([]Edge, edgeCount)
	for i := 0; i < int(edgeCount); i++ {
		row := buffers.Edges[i*EdgeRowSize : (i+1)*EdgeRowSize]
		edge, err := decodeEdgeRow(buffers.Arena, row, nodeCount, idByRow)
		if err != nil {
			return nil, fmt.Errorf("edge row %d error: %w", i, err)
		}
		edges[i] = edge
	}
	return edges, nil
}

func decodeEdgeRow(arena, row []byte, nodeCount uint32, idByRow []string) (Edge, error) {
	source, err := resolveNodeID(arena, row, EdgeOffsetSourceIdx, EdgeOffsetSourceIDStr, nodeCount, idByRow)
	if err != nil {
		return Edge{}, fmt.Errorf("edge source error: %w", err)
	}

	target, err := resolveNodeID(arena, row, EdgeOffsetTargetIdx, EdgeOffsetTargetIDStr, nodeCount, idByRow)
	if err != nil {
		return Edge{}, fmt.Errorf("edge target error: %w", err)
	}

	kindByte := row[EdgeOffsetKind]
	if int(kindByte) >= len(EdgeKinds) {
		return Edge{}, fmt.Errorf("unknown EdgeKind index: %d", kindByte)
	}

	provByte := row[EdgeOffsetProvenance]
	if int(provByte) >= len(Provenances) {
		return Edge{}, fmt.Errorf("unknown provenance index: %d", provByte)
	}

	edge := Edge{
		Source: source,
		Target: target,
		Kind:   EdgeKinds[kindByte],
		Line:   u32opt(row, EdgeOffsetLine),
		Column: u32opt(row, EdgeOffsetColumn),
	}
	if provByte != 0 {
		edge.Provenance = &Provenances[provByte]
	}

	metaJSON, err := readStr(arena, row, EdgeOffsetMetadataJSON)
	if err != nil {
		return Edge{}, err
	}
	if metaJSON != nil {
		var metaMap map[string]any
		if err := json.Unmarshal([]byte(*metaJSON), &metaMap); err != nil {
			return Edge{}, fmt.Errorf("edge metadata invalid JSON: %w", err)
		}
		edge.Metadata = metaMap
	}
	return edge, nil
}

func resolveNodeID(arena, row []byte, idxOffset, strOffset int, nodeCount uint32, idByRow []string) (string, error) {
	idx := binary.LittleEndian.Uint32(row[idxOffset:])
	if idx != NoneSentinel {
		if idx >= nodeCount {
			return "", fmt.Errorf("row index %d out of bounds (nodeCount=%d)", idx, nodeCount)
		}
		return idByRow[idx], nil
	}
	s, err := readStr(arena, row, strOffset)
	if err != nil {
		return "", err
	}
	if s == nil {
		return "", errors.New("absent string endpoint")
	}
	return *s, nil
}

func decodeRefs(buffers Buffers, refCount, nodeCount uint32, idByRow []string, filePath string) ([]UnresolvedReference, error) {
	refs := make([]UnresolvedReference, refCount)
	for i := 0; i < int(refCount); i++ {
		row := buffers.Refs[i*RefRowSize : (i+1)*RefRowSize]
		ref, err := decodeRefRow(buffers.Arena, row, nodeCount, idByRow, filePath)
		if err != nil {
			return nil, fmt.Errorf("ref row %d error: %w", i, err)
		}
		refs[i] = ref
	}
	return refs, nil
}

func decodeRefRow(arena, row []byte, nodeCount uint32, idByRow []string, filePath string) (UnresolvedReference, error) {
	fromNodeID, err := resolveNodeID(arena, row, RefOffsetFromIdx, RefOffsetFromIDStr, nodeCount, idByRow)
	if err != nil {
		return UnresolvedReference{}, fmt.Errorf("ref fromNodeId error: %w", err)
	}

	kindByte := row[RefOffsetKind]
	var refKind string
	if kindByte == FunctionRefCode {
		refKind = "function_ref"
	} else if int(kindByte) < len(EdgeKinds) {
		refKind = EdgeKinds[kindByte]
	} else {
		return UnresolvedReference{}, fmt.Errorf("unknown ReferenceKind index: %d", kindByte)
	}

	refNamePtr, err := readStr(arena, row, RefOffsetReferenceName)
	if err != nil || refNamePtr == nil {
		return UnresolvedReference{}, errors.New("ref missing referenceName")
	}

	ref := UnresolvedReference{
		FromNodeId:    fromNodeID,
		ReferenceName: *refNamePtr,
		ReferenceKind: refKind,
		Line:          int(binary.LittleEndian.Uint32(row[RefOffsetLine:])),
		Column:        int(binary.LittleEndian.Uint32(row[RefOffsetColumn:])),
	}

	flags := row[RefOffsetFlags]
	if (flags & RefFlagFilePath) != 0 {
		ref.FilePath = &filePath
	}

	candidates, err := readStrList(arena, row, RefOffsetCandidates)
	if err != nil {
		return UnresolvedReference{}, err
	}
	if candidates != nil {
		ref.Candidates = candidates
	}
	return ref, nil
}

func decodeErrors(arena []byte, errorsOff, errorsLen uint32) ([]ExtractionError, error) {
	if errorsOff == NoneSentinel {
		return make([]ExtractionError, 0), nil
	}
	if uint64(errorsOff)+uint64(errorsLen) > uint64(len(arena)) {
		return nil, fmt.Errorf("errors offset out of bounds: off=%d, len=%d, arenaLen=%d", errorsOff, errorsLen, len(arena))
	}
	slice := arena[int(errorsOff):int(uint64(errorsOff)+uint64(errorsLen))]
	if !utf8.Valid(slice) {
		return nil, errors.New("invalid utf-8 in errors JSON")
	}

	var extErrors []ExtractionError
	if err := json.Unmarshal(slice, &extErrors); err != nil {
		return nil, fmt.Errorf("errors JSON parse failed: %w", err)
	}
	if extErrors == nil {
		extErrors = make([]ExtractionError, 0)
	}
	return extErrors, nil
}
