package kernel

import (
	"bytes"
	"encoding/binary"
	"math"
	"testing"
)

func TestRejectsInvalidDuration(t *testing.T) {
	for _, value := range []float64{math.NaN(), math.Inf(1), math.Inf(-1), -1} {
		buffers := newBufferBuilder().build(nil, 0)
		binary.LittleEndian.PutUint64(buffers.Meta[MetaOffsetDurationMs:], math.Float64bits(value))
		if _, err := Decode(buffers, "x.go", "go"); err == nil {
			t.Errorf("accepted invalid duration %v", value)
		}
	}
}

func TestRejectsUndeclaredTableBytes(t *testing.T) {
	for _, field := range []string{"nodes", "edges", "refs", "arena"} {
		t.Run(field, func(t *testing.T) {
			buffers := newBufferBuilder().build(nil, 0)
			switch field {
			case "nodes":
				buffers.Nodes = []byte{0}
			case "edges":
				buffers.Edges = []byte{0}
			case "refs":
				buffers.Refs = []byte{0}
			case "arena":
				buffers.Arena = []byte{0}
			}
			if _, err := Decode(buffers, "x.go", "go"); err == nil {
				t.Fatal("accepted bytes not declared by the metadata")
			}
		})
	}
}

func TestArenaReferenceOverflowDoesNotPanic(t *testing.T) {
	row := make([]byte, NodeRowSize)
	binary.LittleEndian.PutUint32(row[NodeOffsetName:], math.MaxUint32-2)
	binary.LittleEndian.PutUint32(row[NodeOffsetName+4:], 10)
	if _, err := readStr(bytes.Repeat([]byte{'x'}, 20), row, NodeOffsetName); err == nil {
		t.Fatal("accepted a wrapping arena reference")
	}
}

func TestErrorsReferenceOverflowDoesNotPanic(t *testing.T) {
	if _, err := decodeErrors([]byte("[]"), math.MaxUint32-1, 5); err == nil {
		t.Fatal("accepted a wrapping errors reference")
	}
}
