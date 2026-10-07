//go:build codegraph_native && cgo

package kernel

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"sync"
	"testing"
)

func TestNativeContractAndGrammars(t *testing.T) {
	if NativeABIVersion() != CABIVersion {
		t.Fatal("C ABI version mismatch")
	}
	oracle := loadNAPIOracle(t)
	contract, err := NativeContract()
	if err != nil {
		t.Fatal(err)
	}
	expected, _ := json.Marshal(oracle.Contract)
	requireJSONEqual(t, expected, contract)
	for language, expectedGrammar := range oracle.Grammars {
		t.Run(language, func(t *testing.T) {
			grammar, err := NativeGrammar(language)
			if err != nil {
				t.Fatal(err)
			}
			expected, _ := json.Marshal(expectedGrammar)
			requireJSONEqual(t, expected, grammar)
		})
	}
	_, err = NativeGrammar("not-a-language")
	if !errors.Is(err, ErrUnsupportedLanguage) {
		t.Fatalf("unknown grammar: %v", err)
	}
}

// Both the raw ABI tables and the complete decoded objects must match. Only
// timing fields are normalized; declarations, metadata, ordering and errors are not.
func TestNativeNAPIParity(t *testing.T) {
	oracle := loadNAPIOracle(t)
	for _, entry := range oracle.Cases {
		t.Run(entry.FilePath, func(t *testing.T) {
			buffers, err := ExtractBuffers(entry.FilePath, entry.Content, entry.Language)
			if entry.Error != "" {
				var nativeErr *NativeError
				if !errors.As(err, &nativeErr) || nativeErr.Status != 3 || nativeErr.Message != entry.Error {
					t.Fatalf("expected extraction error %q, got %v", entry.Error, err)
				}
				return
			}
			if err != nil {
				t.Fatal(err)
			}
			// Copying the buffers before cg_result_free also makes mutation here safe.
			clear(buffers.Meta[28:36])
			for name, table := range map[string][2][]byte{
				"meta": {entry.Buffers.Meta, buffers.Meta}, "nodes": {entry.Buffers.Nodes, buffers.Nodes},
				"edges": {entry.Buffers.Edges, buffers.Edges}, "refs": {entry.Buffers.Refs, buffers.Refs},
				"arena": {entry.Buffers.Arena, buffers.Arena},
			} {
				if !bytes.Equal(table[0], table[1]) {
					t.Errorf("%s differs (%d vs %d bytes)", name, len(table[0]), len(table[1]))
				}
			}
			result, err := DecodeAt(buffers, entry.FilePath, entry.Language, 0)
			if err != nil {
				t.Fatal(err)
			}
			requireJSONEqual(t, entry.Expected, result)
		})
	}
}

func TestNativeCFunctionPointerParity(t *testing.T) {
	oracle := loadNAPIOracle(t)
	for i, entry := range oracle.Cfnptr {
		t.Run(fmt.Sprint(i), func(t *testing.T) {
			facts, err := ScanCFunctionPointers(entry.Input)
			if err != nil {
				t.Fatal(err)
			}
			requireJSONEqual(t, entry.Expected, facts)
		})
	}
	for i, entry := range oracle.Strips {
		t.Run("strip-"+fmt.Sprint(i), func(t *testing.T) {
			stripped, err := StripCComments(entry.Input)
			if err != nil {
				t.Fatal(err)
			}
			if stripped != entry.Expected {
				t.Fatalf("strip differs: want %q, got %q", entry.Expected, stripped)
			}
		})
	}
}

func TestNativeRejectsInvalidArguments(t *testing.T) {
	for _, arg := range []struct {
		path, content, language string
		want                    error
	}{
		{"x.go", "", "yaml", ErrUnsupportedLanguage},
		{"x.go", string([]byte{0xff}), "go", ErrInvalidArgument},
		{string([]byte{0xff}), "", "go", ErrInvalidArgument},
		{"x.go", "", string([]byte{0xff}), ErrInvalidArgument},
	} {
		_, err := Extract(arg.path, arg.content, arg.language)
		if !errors.Is(err, arg.want) {
			t.Errorf("expected %v, got %v", arg.want, err)
		}
	}
}

func TestNativeOwnershipAndConcurrency(t *testing.T) {
	const source = "package main\nfunc hello() { println(\"您好\") }\n"
	first, err := ExtractBuffers("hello.go", source, "go")
	if err != nil {
		t.Fatal(err)
	}
	savedNodes := bytes.Clone(first.Nodes)
	var group sync.WaitGroup
	for worker := 0; worker < 8; worker++ {
		group.Add(1)
		go func() {
			defer group.Done()
			for i := 0; i < 32; i++ {
				result, err := Extract("hello.go", source, "go")
				if err != nil || result == nil {
					t.Errorf("concurrent extraction failed: %v", err)
					return
				}
			}
		}()
	}
	group.Wait()
	if !bytes.Equal(first.Nodes, savedNodes) {
		t.Fatal("Go buffers changed after other Rust results were allocated and released")
	}
}

func TestNativeContractDoesNotExposeCache(t *testing.T) {
	first, err := NativeContract()
	if err != nil {
		t.Fatal(err)
	}
	first.NodeKinds[0] = "mutated"
	first.EdgeKinds[0] = "mutated"
	first.Languages[0] = "mutated"
	second, err := NativeContract()
	if err != nil {
		t.Fatal(err)
	}
	if second.NodeKinds[0] == "mutated" || second.EdgeKinds[0] == "mutated" || second.Languages[0] == "mutated" {
		t.Fatal("a caller mutated the validated native contract cache")
	}
}

func TestNativeScanDoesNotMutateCaller(t *testing.T) {
	input := []CfnptrFile{{Text: "", Structs: nil}}
	if _, err := ScanCFunctionPointers(input); err != nil {
		t.Fatal(err)
	}
	if input[0].Structs != nil {
		t.Fatal("scan normalized the caller's slice in place")
	}
}
