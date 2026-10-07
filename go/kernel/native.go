//go:build codegraph_native && cgo

package kernel

/*
#cgo CFLAGS: -I${SRCDIR}/../../codegraph-kernel/include
#cgo LDFLAGS: ${SRCDIR}/../../codegraph-kernel/target/release/libcodegraph_kernel.a
#cgo darwin LDFLAGS: -lc++
#cgo linux LDFLAGS: -ldl -lm -lpthread -lstdc++
#cgo windows LDFLAGS: -lws2_32 -luserenv -lbcrypt -lntdll -lstdc++
#include "codegraph.h"
*/
import "C"

import (
	"encoding/json"
	"fmt"
	"math"
	"runtime"
	"slices"
	"sync"
	"unsafe"
)

const (
	slotMeta = iota
	slotNodes
	slotEdges
	slotRefs
	slotArena
	slotPayload
	slotError
)

func NativeABIVersion() uint32 {
	return uint32(C.cg_abi_version())
}

// A static library cannot change during a process lifetime. Validate both
// versions and the enum tables once before any extraction or grammar call.
var loadNativeContract = sync.OnceValues(func() (*Contract, error) {
	if version := NativeABIVersion(); version != CABIVersion {
		return nil, fmt.Errorf("codegraph C ABI %d != expected %d", version, CABIVersion)
	}
	data, err := nativePayload(C.cg_contract_info())
	if err != nil {
		return nil, err
	}
	var contract Contract
	if err := json.Unmarshal(data, &contract); err != nil {
		return nil, fmt.Errorf("decode codegraph contract: %w", err)
	}
	if err := ValidateContract(&contract); err != nil {
		return nil, err
	}
	return &contract, nil
})

func checkNativeABI() error {
	_, err := loadNativeContract()
	return err
}

// stringPointer borrows immutable Go string storage only for the synchronous
// C call. Rust never stores these pointers. KeepAlive marks the end of the borrow.
func stringPointer(value string) *C.uint8_t {
	return (*C.uint8_t)(unsafe.Pointer(unsafe.StringData(value)))
}

func copyNativeBuffer(handle *C.cg_result, slot int) ([]byte, error) {
	view := C.cg_result_buffer(handle, C.uint32_t(slot))
	if view.len == 0 {
		return []byte{}, nil
	}
	if view.data == nil || uint64(view.len) > math.MaxInt32 {
		return nil, fmt.Errorf("invalid codegraph C buffer in slot %d (length %d)", slot, uint64(view.len))
	}
	// Every returned slice owns Go memory; no caller observes Rust storage after
	// cg_result_free. GoBytes uses a signed C int length, checked above.
	return C.GoBytes(unsafe.Pointer(view.data), C.int(view.len)), nil
}

func checkNativeResult(handle *C.cg_result) error {
	if handle == nil {
		return fmt.Errorf("codegraph kernel returned a null result")
	}
	status := int32(C.cg_result_status(handle))
	if status == 0 {
		return nil
	}
	message, err := copyNativeBuffer(handle, slotError)
	if err != nil {
		return err
	}
	return &NativeError{Status: status, Message: string(message)}
}

func nativePayload(handle *C.cg_result) ([]byte, error) {
	defer C.cg_result_free(handle)
	if err := checkNativeResult(handle); err != nil {
		return nil, err
	}
	return copyNativeBuffer(handle, slotPayload)
}

func NativeContract() (*Contract, error) {
	contract, err := loadNativeContract()
	if err != nil {
		return nil, err
	}
	// Callers may edit their copy without corrupting the validated cache.
	copy := *contract
	copy.NodeKinds = slices.Clone(contract.NodeKinds)
	copy.EdgeKinds = slices.Clone(contract.EdgeKinds)
	copy.Languages = slices.Clone(contract.Languages)
	return &copy, nil
}

func NativeGrammar(language string) (*GrammarInfo, error) {
	if err := checkNativeABI(); err != nil {
		return nil, err
	}
	handle := C.cg_grammar_info(stringPointer(language), C.size_t(len(language)))
	runtime.KeepAlive(language)
	data, err := nativePayload(handle)
	if err != nil {
		return nil, err
	}
	var grammar GrammarInfo
	if err := json.Unmarshal(data, &grammar); err != nil {
		return nil, fmt.Errorf("decode codegraph grammar: %w", err)
	}
	return &grammar, nil
}

// ExtractBuffers calls the shared Rust extractor and copies all five tables
// before releasing the opaque result. Calls do not share a parser or result.
func ExtractBuffers(filePath, content, language string) (Buffers, error) {
	if err := checkNativeABI(); err != nil {
		return Buffers{}, err
	}
	handle := C.cg_extract_file(
		stringPointer(filePath), C.size_t(len(filePath)),
		stringPointer(content), C.size_t(len(content)),
		stringPointer(language), C.size_t(len(language)),
	)
	runtime.KeepAlive(filePath)
	runtime.KeepAlive(content)
	runtime.KeepAlive(language)
	defer C.cg_result_free(handle)
	if err := checkNativeResult(handle); err != nil {
		return Buffers{}, err
	}
	var buffers Buffers
	for slot, target := range []*[]byte{&buffers.Meta, &buffers.Nodes, &buffers.Edges, &buffers.Refs, &buffers.Arena} {
		data, err := copyNativeBuffer(handle, slot)
		if err != nil {
			return Buffers{}, err
		}
		*target = data
	}
	return buffers, nil
}

func Extract(filePath, content, language string) (*ExtractionResult, error) {
	buffers, err := ExtractBuffers(filePath, content, language)
	if err != nil {
		return nil, err
	}
	return Decode(buffers, filePath, language)
}

func ScanCFunctionPointers(files []CfnptrFile) ([]CfnptrFacts, error) {
	if err := checkNativeABI(); err != nil {
		return nil, err
	}
	// A nil Go slice denotes an empty batch, not a JSON null argument.
	if files == nil {
		files = []CfnptrFile{}
	}
	for i := range files {
		if files[i].Structs == nil {
			// Do not mutate slices owned by the caller while normalizing JSON.
			files = append([]CfnptrFile(nil), files...)
			break
		}
	}
	for i := range files {
		if files[i].Structs == nil {
			files[i].Structs = []CfnptrStruct{}
		}
	}
	input, err := json.Marshal(files)
	if err != nil {
		return nil, err
	}
	handle := C.cg_cfnptr_scan_files((*C.uint8_t)(unsafe.Pointer(unsafe.SliceData(input))), C.size_t(len(input)))
	runtime.KeepAlive(input)
	data, err := nativePayload(handle)
	if err != nil {
		return nil, err
	}
	var facts []CfnptrFacts
	if err := json.Unmarshal(data, &facts); err != nil {
		return nil, fmt.Errorf("decode cFnPtr facts: %w", err)
	}
	return facts, nil
}

func StripCComments(text string) (string, error) {
	if err := checkNativeABI(); err != nil {
		return "", err
	}
	handle := C.cg_cfnptr_strip_c(stringPointer(text), C.size_t(len(text)))
	runtime.KeepAlive(text)
	data, err := nativePayload(handle)
	if err != nil {
		return "", err
	}
	return string(data), nil
}
