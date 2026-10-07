package kernel

import (
	"encoding/json"
)

// Buffers holds the raw flat byte buffers emitted by the native kernel.
type Buffers struct {
	Meta  []byte
	Nodes []byte
	Edges []byte
	Refs  []byte
	Arena []byte
}

// Node represents a code symbol in the knowledge graph.
type Node struct {
	ID             string                     `json:"id"`
	Kind           string                     `json:"kind"`
	Name           string                     `json:"name"`
	QualifiedName  string                     `json:"qualifiedName"`
	FilePath       string                     `json:"filePath"`
	Language       string                     `json:"language"`
	StartLine      int                        `json:"startLine"`
	EndLine        int                        `json:"endLine"`
	StartColumn    int                        `json:"startColumn"`
	EndColumn      int                        `json:"endColumn"`
	Docstring      *string                    `json:"docstring,omitempty"`
	Signature      *string                    `json:"signature,omitempty"`
	Visibility     *string                    `json:"visibility,omitempty"`
	IsExported     *bool                      `json:"isExported,omitempty"`
	IsAsync        *bool                      `json:"isAsync,omitempty"`
	IsStatic       *bool                      `json:"isStatic,omitempty"`
	IsAbstract     *bool                      `json:"isAbstract,omitempty"`
	Decorators     []string                   `json:"decorators,omitempty"`
	TypeParameters []string                   `json:"typeParameters,omitempty"`
	ReturnType     *string                    `json:"returnType,omitempty"`
	UpdatedAt      int64                      `json:"updatedAt"`
	Extra          map[string]json.RawMessage `json:"-"`
}

// MarshalJSON serializes Node with Object.assign semantics for Extra fields.
func (n Node) MarshalJSON() ([]byte, error) {
	m := make(map[string]any)
	m["id"] = n.ID
	m["kind"] = n.Kind
	m["name"] = n.Name
	m["qualifiedName"] = n.QualifiedName
	m["filePath"] = n.FilePath
	m["language"] = n.Language
	m["startLine"] = n.StartLine
	m["endLine"] = n.EndLine
	m["startColumn"] = n.StartColumn
	m["endColumn"] = n.EndColumn
	m["updatedAt"] = n.UpdatedAt

	if n.Docstring != nil {
		m["docstring"] = *n.Docstring
	}
	if n.Signature != nil {
		m["signature"] = *n.Signature
	}
	if n.Visibility != nil {
		m["visibility"] = *n.Visibility
	}
	if n.IsExported != nil {
		m["isExported"] = *n.IsExported
	}
	if n.IsAsync != nil {
		m["isAsync"] = *n.IsAsync
	}
	if n.IsStatic != nil {
		m["isStatic"] = *n.IsStatic
	}
	if n.IsAbstract != nil {
		m["isAbstract"] = *n.IsAbstract
	}
	if n.Decorators != nil {
		m["decorators"] = n.Decorators
	}
	if n.TypeParameters != nil {
		m["typeParameters"] = n.TypeParameters
	}
	if n.ReturnType != nil {
		m["returnType"] = *n.ReturnType
	}

	for k, v := range n.Extra {
		m[k] = v
	}
	return json.Marshal(m)
}

// UnmarshalJSON deserializes Node, populating strong fields and Extra.
func (n *Node) UnmarshalJSON(data []byte) error {
	var raw map[string]json.RawMessage
	if err := json.Unmarshal(data, &raw); err != nil {
		return err
	}
	type shadow Node
	var s shadow
	if err := json.Unmarshal(data, &s); err != nil {
		return err
	}
	*n = Node(s)

	knownKeys := map[string]bool{
		"id": true, "kind": true, "name": true, "qualifiedName": true,
		"filePath": true, "language": true, "startLine": true, "endLine": true,
		"startColumn": true, "endColumn": true, "docstring": true, "signature": true,
		"visibility": true, "isExported": true, "isAsync": true, "isStatic": true,
		"isAbstract": true, "decorators": true, "typeParameters": true,
		"returnType": true, "updatedAt": true,
	}

	for k, v := range raw {
		if knownKeys[k] {
			continue
		}
		if n.Extra == nil {
			n.Extra = make(map[string]json.RawMessage)
		}
		n.Extra[k] = v
	}
	return nil
}

// Edge represents a relationship between two nodes.
type Edge struct {
	Source     string         `json:"source"`
	Target     string         `json:"target"`
	Kind       string         `json:"kind"`
	Metadata   map[string]any `json:"metadata,omitempty"`
	Line       *int           `json:"line,omitempty"`
	Column     *int           `json:"column,omitempty"`
	Provenance *string        `json:"provenance,omitempty"`
}

// MarshalJSON serializes Edge preserving line/column 0 and empty metadata objects.
func (e Edge) MarshalJSON() ([]byte, error) {
	m := make(map[string]any)
	m["source"] = e.Source
	m["target"] = e.Target
	m["kind"] = e.Kind

	if e.Metadata != nil {
		m["metadata"] = e.Metadata
	}
	if e.Line != nil {
		m["line"] = *e.Line
	}
	if e.Column != nil {
		m["column"] = *e.Column
	}
	if e.Provenance != nil {
		m["provenance"] = *e.Provenance
	}
	return json.Marshal(m)
}

// UnresolvedReference represents a reference to be resolved later.
type UnresolvedReference struct {
	FromNodeId    string   `json:"fromNodeId"`
	ReferenceName string   `json:"referenceName"`
	ReferenceKind string   `json:"referenceKind"`
	Line          int      `json:"line"`
	Column        int      `json:"column"`
	FilePath      *string  `json:"filePath,omitempty"`
	Language      *string  `json:"language,omitempty"`
	Candidates    []string `json:"candidates,omitempty"`
	RowID         *int64   `json:"rowId,omitempty"`
}

// MarshalJSON ensures Candidates distinguishes absent (nil) from empty ([]).
func (r UnresolvedReference) MarshalJSON() ([]byte, error) {
	m := make(map[string]any)
	m["fromNodeId"] = r.FromNodeId
	m["referenceName"] = r.ReferenceName
	m["referenceKind"] = r.ReferenceKind
	m["line"] = r.Line
	m["column"] = r.Column

	if r.FilePath != nil {
		m["filePath"] = *r.FilePath
	}
	if r.Language != nil {
		m["language"] = *r.Language
	}
	if r.Candidates != nil {
		m["candidates"] = r.Candidates
	}
	if r.RowID != nil {
		m["rowId"] = *r.RowID
	}
	return json.Marshal(m)
}

// ExtractionError records an error or warning encountered during extraction.
type ExtractionError struct {
	Message  string  `json:"message"`
	FilePath *string `json:"filePath,omitempty"`
	Line     *int    `json:"line,omitempty"`
	Column   *int    `json:"column,omitempty"`
	Severity string  `json:"severity"`
	Code     *string `json:"code,omitempty"`
}

// ExtractionResult is the complete output from parsing a source file.
type ExtractionResult struct {
	Nodes                []Node                `json:"nodes"`
	Edges                []Edge                `json:"edges"`
	UnresolvedReferences []UnresolvedReference `json:"unresolvedReferences"`
	Errors               []ExtractionError     `json:"errors"`
	DurationMs           float64               `json:"durationMs"`
}

// MarshalJSON guarantees arrays are serialized as [] instead of null.
func (res *ExtractionResult) MarshalJSON() ([]byte, error) {
	type shadow ExtractionResult
	s := shadow(*res)
	if s.Nodes == nil {
		s.Nodes = make([]Node, 0)
	}
	if s.Edges == nil {
		s.Edges = make([]Edge, 0)
	}
	if s.UnresolvedReferences == nil {
		s.UnresolvedReferences = make([]UnresolvedReference, 0)
	}
	if s.Errors == nil {
		s.Errors = make([]ExtractionError, 0)
	}
	return json.Marshal(s)
}

// Helper functions for pointers.
func BoolPtr(b bool) *bool       { return &b }
func IntPtr(v int) *int          { return &v }
func Int64Ptr(v int64) *int64    { return &v }
func StringPtr(s string) *string { return &s }
