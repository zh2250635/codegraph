package kernel

import (
	"errors"
	"fmt"
)

// C ABI versions and extraction buffer versions are independent contracts.
const CABIVersion = 1

var (
	ErrInvalidArgument     = errors.New("invalid native kernel argument")
	ErrUnsupportedLanguage = errors.New("unsupported native kernel language")
	ErrExtraction          = errors.New("native kernel extraction failed")
	ErrNativePanic         = errors.New("native kernel panic")
)

// NativeError preserves the C ABI status and the diagnostic supplied by Rust.
type NativeError struct {
	Status  int32
	Message string
}

func (e *NativeError) Error() string {
	return fmt.Sprintf("codegraph kernel status %d: %s", e.Status, e.Message)
}

func (e *NativeError) Unwrap() error {
	switch e.Status {
	case 1:
		return ErrInvalidArgument
	case 2:
		return ErrUnsupportedLanguage
	case 3:
		return ErrExtraction
	case 4:
		return ErrNativePanic
	default:
		return nil
	}
}

// CfnptrStruct is a struct's inclusive source extent. EndLine must already have
// the caller's "endLine ?? startLine" fallback applied, matching the TS caller.
type CfnptrStruct struct {
	ID        string `json:"id"`
	StartLine uint32 `json:"startLine"`
	EndLine   uint32 `json:"endLine"`
}

type CfnptrFile struct {
	Text    string         `json:"text"`
	Structs []CfnptrStruct `json:"structs"`
}

type CfnptrField struct {
	Name  string `json:"name"`
	Index uint32 `json:"index"`
	Ptr   bool   `json:"ptr"`
	Type  string `json:"type"`
}

type CfnptrStructFacts struct {
	ID     string        `json:"id"`
	Parsed bool          `json:"parsed"`
	Fields []CfnptrField `json:"fields"`
}

type CfnptrFacts struct {
	FnPtrTypedefs      []string            `json:"fnPtrTypedefs"`
	FnTypeTypedefs     []string            `json:"fnTypeTypedefs"`
	Structs            []CfnptrStructFacts `json:"structs"`
	InlinePtr          bool                `json:"inlinePtr"`
	InlineTypes        []string            `json:"inlineTypes"`
	InlineTags         []string            `json:"inlineTags"`
	InitTokens         []string            `json:"initTokens"`
	ArrayElems         []string            `json:"arrayElems"`
	AliasNames         []string            `json:"aliasNames"`
	DPairs             []string            `json:"dPairs"`
	DispatchFields     []string            `json:"dispatchFields"`
	ArrayDispatchNames []string            `json:"arrayDispatchNames"`
	Includes           []string            `json:"includes"`
}
