// Package daemonclient is a wrapper around a grpc client
// to talk to turbod
package daemonclient

import (
	"context"

	"github.com/vercel/turborepo/cli/internal/daemon/connector"
	"github.com/vercel/turborepo/cli/internal/turbodprotocol"
	"github.com/vercel/turborepo/cli/internal/turbopath"
)

// DaemonClient provides access to higher-level functionality from the daemon to a turbo run.
type DaemonClient struct {
	client *connector.Client
}

// Status provides details about the daemon's status
type Status struct {
	UptimeMs uint64                 `json:"uptimeMs"`
	LogFile  turbopath.AbsolutePath `json:"logFile"`
	PidFile  turbopath.AbsolutePath `json:"pidFile"`
	SockFile turbopath.AbsolutePath `json:"sockFile"`
}

// New creates a new instance of a DaemonClient.
func New(client *connector.Client) *DaemonClient {
	return &DaemonClient{
		client: client,
	}
}

// GetChangedOutputs implements runcache.OutputWatcher.GetChangedOutputs
func (d *DaemonClient) GetChangedOutputs(ctx context.Context, hash string, repoRelativeOutputGlobs []string) ([]string, error) {
	resp, err := d.client.GetChangedOutputs(ctx, &turbodprotocol.GetChangedOutputsRequest{
		Hash:        hash,
		OutputGlobs: repoRelativeOutputGlobs,
	})
	if err != nil {
		return nil, err
	}
	return resp.ChangedOutputGlobs, nil
}

// NotifyOutputsWritten implements runcache.OutputWatcher.NotifyOutputsWritten
func (d *DaemonClient) NotifyOutputsWritten(ctx context.Context, hash string, repoRelativeOutputGlobs []string) error {
	_, err := d.client.NotifyOutputsWritten(ctx, &turbodprotocol.NotifyOutputsWrittenRequest{
		Hash:        hash,
		OutputGlobs: repoRelativeOutputGlobs,
	})
	return err
}

// Status returns the DaemonStatus from the daemon
func (d *DaemonClient) Status(ctx context.Context) (*Status, error) {
	resp, err := d.client.Status(ctx, &turbodprotocol.StatusRequest{})
	if err != nil {
		return nil, err
	}
	daemonStatus := resp.DaemonStatus
	return &Status{
		UptimeMs: daemonStatus.UptimeMsec,
		LogFile:  d.client.LogPath,
		PidFile:  d.client.PidPath,
		SockFile: d.client.SockPath,
	}, nil
}
