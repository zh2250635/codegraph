package kernel

import (
	"encoding/json"
	"fmt"
	"os"
	"reflect"
	"sort"
	"testing"
)

type oracleCase struct {
	FilePath, Content, Language, Error string
	Expected                           json.RawMessage
	Buffers                            Buffers
}

type napiOracle struct {
	Release, SourceRevision string
	Contract                Contract
	Grammars                map[string]GrammarInfo
	Cases                   []oracleCase
	Cfnptr                  []struct {
		Input    []CfnptrFile
		Expected json.RawMessage
	}
	Strips []struct {
		Input, Expected string
	}
}

func loadNAPIOracle(t *testing.T) napiOracle {
	t.Helper()
	data, err := os.ReadFile("testdata/napi-v1.6.0.json")
	if err != nil {
		t.Fatal(err)
	}
	var oracle napiOracle
	if err := json.Unmarshal(data, &oracle); err != nil {
		t.Fatal(err)
	}
	if oracle.Release != "1.6.0" || oracle.SourceRevision != "dfccdf62547fcd76d343344d823a0e1998d3a89f" {
		t.Fatal("unexpected oracle release or source revision")
	}
	if len(oracle.Contract.Languages) != 20 || len(oracle.Cases) != 62 {
		t.Fatal("oracle corpus coverage changed; review the capture before changing this gate")
	}
	return oracle
}

func requireJSONEqual(t *testing.T, expected json.RawMessage, actual any) {
	t.Helper()
	data, err := json.Marshal(actual)
	if err != nil {
		t.Fatal(err)
	}
	var want, got any
	if err := json.Unmarshal(expected, &want); err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(data, &got); err != nil {
		t.Fatal(err)
	}
	if difference := firstJSONDifference(want, got, "$"); difference != "" {
		t.Error(difference)
	}
}

func firstJSONDifference(want, got any, path string) string {
	if reflect.DeepEqual(want, got) {
		return ""
	}
	switch w := want.(type) {
	case map[string]any:
		g, ok := got.(map[string]any)
		if !ok {
			return path + ": expected an object"
		}
		keys := make([]string, 0, len(w))
		for key := range w {
			keys = append(keys, key)
		}
		sort.Strings(keys)
		for _, key := range keys {
			value, exists := g[key]
			if !exists {
				return path + "." + key + ": missing field"
			}
			if diff := firstJSONDifference(w[key], value, path+"."+key); diff != "" {
				return diff
			}
		}
		for key := range g {
			if _, exists := w[key]; !exists {
				return path + "." + key + ": unexpected field"
			}
		}
	case []any:
		g, ok := got.([]any)
		if !ok || len(w) != len(g) {
			return fmt.Sprintf("%s: array length/type differs", path)
		}
		for i := range w {
			if diff := firstJSONDifference(w[i], g[i], fmt.Sprintf("%s[%d]", path, i)); diff != "" {
				return diff
			}
		}
	default:
		return fmt.Sprintf("%s: expected %.200v, got %.200v", path, want, got)
	}
	return path + ": JSON differs"
}

// This test needs neither Node nor a native library. It exercises the Go
// decoder against flat buffers and complete decoded objects from the release.
func TestDecoderNAPIReplay(t *testing.T) {
	oracle := loadNAPIOracle(t)
	if err := ValidateContract(&oracle.Contract); err != nil {
		t.Fatal(err)
	}
	languages := make(map[string]bool)
	for _, entry := range oracle.Cases {
		if entry.Error != "" {
			continue
		}
		languages[entry.Language] = true
		t.Run(entry.FilePath, func(t *testing.T) {
			result, err := DecodeAt(entry.Buffers, entry.FilePath, entry.Language, 0)
			if err != nil {
				t.Fatal(err)
			}
			requireJSONEqual(t, entry.Expected, result)
		})
	}
	for _, language := range oracle.Contract.Languages {
		if !languages[language] {
			t.Errorf("no successful decoder oracle for %s", language)
		}
	}
}
