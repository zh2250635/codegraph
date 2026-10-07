package kernel

import (
	"errors"
	"fmt"
)

// Contract describes the wire contract exported by the native kernel.
type Contract struct {
	ABIVersion    uint32   `json:"abiVersion"`
	KernelVersion string   `json:"kernelVersion"`
	NodeKinds     []string `json:"nodeKinds"`
	EdgeKinds     []string `json:"edgeKinds"`
	Languages     []string `json:"languages"`
}

// GrammarInfo describes grammar identity for tree-sitter language bindings.
type GrammarInfo struct {
	ABIVersion    uint32   `json:"abiVersion"`
	NodeKindCount uint32   `json:"nodeKindCount"`
	FieldCount    uint32   `json:"fieldCount"`
	NodeKinds     []string `json:"nodeKinds"`
	FieldNames    []string `json:"fieldNames"`
}

// ValidateContract verifies ABI version, NodeKinds table and EdgeKinds table ordering.
func ValidateContract(c *Contract) error {
	if c == nil {
		return errors.New("contract is nil")
	}
	if c.ABIVersion != uint32(KernelABIVersion) {
		return fmt.Errorf("contract ABI version %d != expected %d", c.ABIVersion, KernelABIVersion)
	}
	if err := checkTableOrder("NodeKinds", c.NodeKinds, NodeKinds); err != nil {
		return err
	}
	return checkTableOrder("EdgeKinds", c.EdgeKinds, EdgeKinds)
}

func checkTableOrder(tableName string, actual, expected []string) error {
	if len(actual) != len(expected) {
		return fmt.Errorf("%s table length mismatch: got %d, expected %d", tableName, len(actual), len(expected))
	}
	for i := range expected {
		if actual[i] != expected[i] {
			return fmt.Errorf("%s mismatch at index %d: got %q, expected %q", tableName, i, actual[i], expected[i])
		}
	}
	return nil
}

var supportedLangsMap = func() map[string]struct{} {
	m := make(map[string]struct{}, len(Languages))
	for _, lang := range Languages {
		m[lang] = struct{}{}
	}
	return m
}()

// SupportedLanguage checks if the given language is known to the system.
func SupportedLanguage(lang string) bool {
	_, ok := supportedLangsMap[lang]
	return ok
}
