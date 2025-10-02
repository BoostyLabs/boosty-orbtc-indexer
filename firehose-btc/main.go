package main

import (
        "context"
        "encoding/hex"
        "flag"
        "fmt"
        "log"
        "os"
        "time"

        pbbtc "buf.build/gen/go/streamingfast/firehose-bitcoin/protocolbuffers/go/sf/bitcoin/type/v1"
        pbfirehose "github.com/streamingfast/pbgo/sf/firehose/v2"

        "github.com/mostynb/go-grpc-compression/zstd"
        "github.com/streamingfast/firehose-core/firehose/client"
        "google.golang.org/grpc"
)

const FirehoseBTC = "mainnet.btc.streamingfast.io:443"

func main() {
        blockNum := flag.Uint64("block", 840000, "")
        parse := flag.Bool("parse", false, "")
        flag.Parse()
        apiKey := os.Getenv("SUBSTREAMS_API_KEY")
        if apiKey == "" {
                panic("SUBSTREAMS_API_KEY env variable must be set")
        }

        fhClient, closeFunc, callOpts, err := client.NewFirehoseFetchClient(FirehoseBTC, "", apiKey, false, false)
        if err != nil {
                log.Panicf("failed to create Firehose client: %s", err)
        }
        defer closeFunc()

        // Optionally you can enable gRPC compression
        callOpts = append(callOpts, grpc.UseCompressor(zstd.Name))

        block, err := fhClient.Block(context.Background(), &pbfirehose.SingleBlockRequest{
                // Request a block by its block number
                Reference: &pbfirehose.SingleBlockRequest_BlockNumber_{
                        BlockNumber: &pbfirehose.SingleBlockRequest_BlockNumber{Num: *blockNum},
                },
        }, callOpts...)
        if err != nil {
                log.Panicf("failed to fetch block: %s", err)
        }


        if *parse {
                var btcBlock pbbtc.Block
                err = block.Block.UnmarshalTo(&btcBlock)
                if err != nil {
                        log.Panicf("failed to decode to Bitcoin block: %s", err)
                }

                fmt.Printf("received block: %d, blocktime: %s, hash: %s, trxs: %d\n",
                        btcBlock.Height,
                        time.Unix(btcBlock.Time, 0),
                        btcBlock.Hash,
                        len(btcBlock.Tx),
                )
        }

        blockData := hex.EncodeToString(block.Block.Value)
        fmt.Printf("<block>:%s\n", blockData)
}
