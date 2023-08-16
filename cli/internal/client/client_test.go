package client

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io/ioutil"
	"net/http"
	"net/http/httptest"
	"reflect"
	"testing"
	"time"

	"github.com/google/uuid"
	"github.com/hashicorp/go-hclog"
	"github.com/vercel/turbo/cli/internal/turbostate"
	"github.com/vercel/turbo/cli/internal/util"
	"gotest.tools/v3/assert"
)

func Test_sendToServer(t *testing.T) {
	ch := make(chan []byte, 1)
	ts := httptest.NewServer(
		http.HandlerFunc(func(w http.ResponseWriter, req *http.Request) {
			defer req.Body.Close()
			b, err := ioutil.ReadAll(req.Body)
			if err != nil {
				t.Errorf("failed to read request %v", err)
			}
			ch <- b
			w.WriteHeader(200)
			w.Write([]byte{})
		}))
	defer ts.Close()

	apiClientConfig := turbostate.APIClientConfig{
		TeamSlug: "my-team-slug",
		APIURL:   ts.URL,
		Token:    "my-token",
	}
	apiClient := NewClient(apiClientConfig, hclog.Default(), "v1")

	myUUID, err := uuid.NewUUID()
	if err != nil {
		t.Errorf("failed to create uuid %v", err)
	}
	events := []map[string]interface{}{
		{
			"sessionId": myUUID.String(),
			"hash":      "foo",
			"source":    "LOCAL",
			"event":     "hit",
		},
		{
			"sessionId": myUUID.String(),
			"hash":      "bar",
			"source":    "REMOTE",
			"event":     "MISS",
		},
	}
	ctx := context.Background()
	err = apiClient.RecordAnalyticsEvents(ctx, events)
	assert.NilError(t, err, "RecordAnalyticsEvent")

	body := <-ch

	result := []map[string]interface{}{}
	err = json.Unmarshal(body, &result)
	if err != nil {
		t.Errorf("unmarshalling body %v", err)
	}
	if !reflect.DeepEqual(events, result) {
		t.Errorf("roundtrip got %v, want %v", result, events)
	}
}

func Test_PutArtifact(t *testing.T) {
	ch := make(chan []byte, 1)
	ts := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, req *http.Request) {
		defer req.Body.Close()
		b, err := ioutil.ReadAll(req.Body)
		if err != nil {
			t.Errorf("failed to read request %v", err)
		}
		ch <- b
		w.WriteHeader(200)
		w.Write([]byte{})
	}))
	defer ts.Close()

	// Set up test expected values
	apiClientConfig := turbostate.APIClientConfig{
		TeamSlug: "my-team-slug",
		APIURL:   ts.URL,
		Token:    "my-token",
	}
	apiClient := NewClient(apiClientConfig, hclog.Default(), "v1")
	expectedArtifactBody := []byte("My string artifact")

	// Test Put Artifact
	apiClient.PutArtifact("hash", expectedArtifactBody, 500, "")
	testBody := <-ch
	if !bytes.Equal(expectedArtifactBody, testBody) {
		t.Errorf("Handler read '%v', wants '%v'", testBody, expectedArtifactBody)
	}

}

func Test_PutWhenCachingDisabled(t *testing.T) {
	ts := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, req *http.Request) {
		defer func() { _ = req.Body.Close() }()
		w.WriteHeader(403)
		_, _ = w.Write([]byte("{\"code\": \"remote_caching_disabled\",\"message\":\"caching disabled\"}"))
	}))
	defer ts.Close()

	// Set up test expected values
	apiClientConfig := turbostate.APIClientConfig{
		TeamSlug: "my-team-slug",
		APIURL:   ts.URL,
		Token:    "my-token",
	}
	apiClient := NewClient(apiClientConfig, hclog.Default(), "v1")
	expectedArtifactBody := []byte("My string artifact")
	// Test Put Artifact
	err := apiClient.PutArtifact("hash", expectedArtifactBody, 500, "")
	cd := &util.CacheDisabledError{}
	if !errors.As(err, &cd) {
		t.Errorf("expected cache disabled error, got %v", err)
	}
	if cd.Status != util.CachingStatusDisabled {
		t.Errorf("caching status: expected %v, got %v", util.CachingStatusDisabled, cd.Status)
	}
}

func Test_FetchWhenCachingDisabled(t *testing.T) {
	ts := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, req *http.Request) {
		defer func() { _ = req.Body.Close() }()
		w.WriteHeader(403)
		_, _ = w.Write([]byte("{\"code\": \"remote_caching_disabled\",\"message\":\"caching disabled\"}"))
	}))
	defer ts.Close()

	// Set up test expected values
	apiClientConfig := turbostate.APIClientConfig{
		TeamSlug: "my-team-slug",
		APIURL:   ts.URL,
		Token:    "my-token",
	}
	apiClient := NewClient(apiClientConfig, hclog.Default(), "v1")
	// Test Put Artifact
	resp, err := apiClient.FetchArtifact("hash")
	cd := &util.CacheDisabledError{}
	if !errors.As(err, &cd) {
		t.Errorf("expected cache disabled error, got %v", err)
	}
	if cd.Status != util.CachingStatusDisabled {
		t.Errorf("caching status: expected %v, got %v", util.CachingStatusDisabled, cd.Status)
	}
	if resp != nil {
		t.Errorf("response got %v, want <nil>", resp)
	}
}

func Test_Timeout(t *testing.T) {
	ts := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, req *http.Request) {
		<-time.After(50 * time.Millisecond)
	}))
	defer ts.Close()

	// Set up test expected values
	apiClientConfig := turbostate.APIClientConfig{
		TeamSlug: "my-team-slug",
		APIURL:   ts.URL,
		Token:    "my-token",
	}
	apiClient := NewClient(apiClientConfig, hclog.Default(), "v1")

	ctx, cancel := context.WithTimeout(context.Background(), 1*time.Millisecond)
	defer cancel()
	_, err := apiClient.JSONPost(ctx, "/", []byte{})
	if !errors.Is(err, context.DeadlineExceeded) {
		t.Errorf("JSONPost got %v, want DeadlineExceeded", err)
	}
}
