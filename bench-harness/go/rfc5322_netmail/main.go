// RFC 5322 message parse via Go's stdlib `net/mail`.
//
// Build: go build -o rfc5322_netmail .
// Run:   ./rfc5322_netmail ../../corpus/rfc5322_message.eml 1000000

package main

import (
	"bytes"
	"fmt"
	"net/mail"
	"os"
	"strconv"
	"time"
)

func main() {
	if len(os.Args) != 3 {
		fmt.Fprintln(os.Stderr, "usage: rfc5322_netmail <corpus.eml> <iterations>")
		os.Exit(1)
	}
	data, err := os.ReadFile(os.Args[1])
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	iters, err := strconv.Atoi(os.Args[2])
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}

	start := time.Now()
	for i := 0; i < iters; i++ {
		_, _ = mail.ReadMessage(bytes.NewReader(data))
	}
	elapsed := time.Since(start)
	nsPerOp := float64(elapsed.Nanoseconds()) / float64(iters)
	fmt.Printf("go/net-mail/read-message: %.1f ns/op (%d iters)\n", nsPerOp, iters)
}
