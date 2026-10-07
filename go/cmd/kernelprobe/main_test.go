//go:build codegraph_native && cgo

package main

import (
	"bytes"
	"encoding/json"
	"errors"
	"io"
	"strings"
	"testing"
)

func TestCommands(t *testing.T) {
	for _, command := range []struct {
		name  string
		args  []string
		input string
	}{
		{"contract", []string{"contract"}, ""},
		{"grammar", []string{"grammar", "go"}, ""},
		{"extract", []string{"extract", "--language", "go", "--path", "main.go"}, "package main\nfunc hello() {}\n"},
		{"scan", []string{"scan-cfnptr"}, "[]"},
	} {
		t.Run(command.name, func(t *testing.T) {
			var output bytes.Buffer
			if err := run(command.args, strings.NewReader(command.input), &output); err != nil {
				t.Fatal(err)
			}
			if !json.Valid(output.Bytes()) {
				t.Fatal("command returned invalid JSON")
			}
		})
	}
}

func TestStrip(t *testing.T) {
	var output bytes.Buffer
	if err := run([]string{"strip-c"}, strings.NewReader("int x; // comment\n"), &output); err != nil {
		t.Fatal(err)
	}
	if strings.Contains(output.String(), "comment") || !strings.Contains(output.String(), "int x;") {
		t.Fatal("invalid strip output")
	}
}

func TestRejectsBadCommands(t *testing.T) {
	for _, args := range [][]string{
		nil, {"missing"}, {"contract", "extra"}, {"grammar"},
		{"extract"}, {"extract", "--language", "go", "--path", "x", "extra"},
		{"strip-c", "extra"}, {"scan-cfnptr", "extra"},
	} {
		if err := run(args, strings.NewReader(""), io.Discard); err == nil {
			t.Errorf("accepted invalid command %v", args)
		}
	}
}

type failedReader struct{}

func (failedReader) Read([]byte) (int, error) {
	return 0, errors.New("read failed")
}

func TestReadFailure(t *testing.T) {
	if _, err := readInput(failedReader{}); err == nil {
		t.Fatal("read failure was swallowed")
	}
}

func TestWriteFailure(t *testing.T) {
	writer := failedWriter{}
	if err := run([]string{"contract"}, strings.NewReader(""), writer); err == nil {
		t.Fatal("write failure was swallowed")
	}
}

type failedWriter struct{}

func (failedWriter) Write([]byte) (int, error) {
	return 0, errors.New("write failed")
}
