//go:build codegraph_native && cgo

// kernelprobe exercises the extraction boundary without the graph service.
// It reads source from stdin and never invokes Node or the TypeScript pipeline.
package main

import (
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"

	"github.com/zh2250635/codegraph/go/kernel"
)

const maxInputBytes = 64 * 1024 * 1024

func main() {
	if err := run(os.Args[1:], os.Stdin, os.Stdout); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func run(args []string, input io.Reader, output io.Writer) error {
	if len(args) == 0 {
		return errors.New("usage: kernelprobe contract | grammar LANGUAGE | extract --language LANGUAGE --path PATH | scan-cfnptr | strip-c")
	}
	switch args[0] {
	case "contract":
		if len(args) != 1 {
			return errors.New("contract takes no arguments")
		}
		contract, err := kernel.NativeContract()
		return writeJSON(output, contract, err)
	case "grammar":
		if len(args) != 2 {
			return errors.New("grammar requires one language")
		}
		grammar, err := kernel.NativeGrammar(args[1])
		return writeJSON(output, grammar, err)
	case "extract":
		return extract(args[1:], input, output)
	case "scan-cfnptr", "strip-c":
		if len(args) != 1 {
			return errors.New(args[0] + " takes no arguments")
		}
		return scanOrStrip(args[0], input, output)
	default:
		return fmt.Errorf("unknown command %q", args[0])
	}
}

func readInput(input io.Reader) ([]byte, error) {
	data, err := io.ReadAll(io.LimitReader(input, maxInputBytes+1))
	if err != nil {
		return nil, err
	}
	if len(data) > maxInputBytes {
		return nil, fmt.Errorf("input exceeds %d bytes", maxInputBytes)
	}
	return data, nil
}

func extract(args []string, input io.Reader, output io.Writer) error {
	flags := flag.NewFlagSet("extract", flag.ContinueOnError)
	flags.SetOutput(io.Discard)
	language := flags.String("language", "", "native language identifier")
	filePath := flags.String("path", "", "logical source path used in symbol IDs")
	if err := flags.Parse(args); err != nil {
		return err
	}
	if *language == "" || *filePath == "" || flags.NArg() != 0 {
		return errors.New("extract requires --language and --path, and reads source from stdin")
	}
	data, err := readInput(input)
	if err != nil {
		return err
	}
	result, err := kernel.Extract(*filePath, string(data), *language)
	return writeJSON(output, result, err)
}

func scanOrStrip(command string, input io.Reader, output io.Writer) error {
	data, err := readInput(input)
	if err != nil {
		return err
	}
	if command == "strip-c" {
		text, err := kernel.StripCComments(string(data))
		if err != nil {
			return err
		}
		_, err = io.WriteString(output, text)
		return err
	}
	var files []kernel.CfnptrFile
	if err := json.Unmarshal(data, &files); err != nil {
		return err
	}
	facts, err := kernel.ScanCFunctionPointers(files)
	return writeJSON(output, facts, err)
}

func writeJSON(output io.Writer, value any, err error) error {
	if err != nil {
		return err
	}
	return json.NewEncoder(output).Encode(value)
}
