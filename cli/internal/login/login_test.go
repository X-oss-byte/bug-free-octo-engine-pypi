package login

import (
	"fmt"
	"net/http"
	"net/url"
	"os"
	"testing"

	"github.com/hashicorp/go-hclog"
	"github.com/pkg/errors"
	"github.com/vercel/turborepo/cli/internal/client"
	"github.com/vercel/turborepo/cli/internal/config"
	"github.com/vercel/turborepo/cli/internal/fs"
	"github.com/vercel/turborepo/cli/internal/turbopath"
	"github.com/vercel/turborepo/cli/internal/ui"
	"github.com/vercel/turborepo/cli/internal/util"
)

type dummyClient struct {
	setToken            string
	createdSSOTokenName string
	team                *client.Team
	cachingStatus       util.CachingStatus
}

func (d *dummyClient) SetToken(t string) {
	d.setToken = t
}

func (d *dummyClient) GetUser() (*client.UserResponse, error) {
	return &client.UserResponse{}, nil
}

func (d *dummyClient) GetTeam(teamID string) (*client.Team, error) {
	return d.team, nil
}

func (d *dummyClient) GetCachingStatus() (util.CachingStatus, error) {
	return d.cachingStatus, nil
}

func (d *dummyClient) SetTeamID(teamID string) {}

func (d *dummyClient) VerifySSOToken(token string, tokenName string) (*client.VerifiedSSOUser, error) {
	d.createdSSOTokenName = tokenName
	return &client.VerifiedSSOUser{
		Token:  "actual-sso-token",
		TeamID: "sso-team-id",
	}, nil
}

var logger = hclog.Default()

func getConfig(t *testing.T) *config.Config {
	t.Helper()
	repoRoot := fs.AbsolutePathFromUpstream(t.TempDir())
	configPath := fs.AbsolutePathFromUpstream(t.TempDir()).Join("turborepo", "config.json")
	userConfig, err := config.ReadUserConfigFile(configPath)
	if err != nil {
		t.Fatalf("failed to load user config: %v", err)
	}
	t.Cleanup(func() {
		_ = os.Unsetenv("TURBO_LOGIN")
		_ = os.Unsetenv("TURBO_API")
	})
	_ = os.Setenv("TURBO_LOGIN", "login-url")
	_ = os.Setenv("TURBO_API", "api-url")
	repoConfig, err := config.ReadRepoConfigFile(config.GetRepoConfigPath(repoRoot))
	if err != nil {
		t.Fatalf("failed to load repo config: %v", err)
	}
	remoteConfig := repoConfig.GetRemoteConfig(userConfig.Token())
	return &config.Config{
		Logger:       logger,
		TurboVersion: "test",
		RepoConfig:   repoConfig,
		LoginURL:     repoConfig.LoginURL(),
		UserConfig:   userConfig,
		RemoteConfig: remoteConfig,
		Cwd:          repoRoot,
	}
}

type testResult struct {
	repoRoot            turbopath.AbsolutePath
	clientErr           error
	openedURL           string
	stepCh              chan struct{}
	client              dummyClient
	shouldEnableCaching bool
}

func (tr *testResult) repoConfigWritten(t *testing.T) *config.RepoConfig {
	config, err := config.ReadRepoConfigFile(config.GetRepoConfigPath(tr.repoRoot))
	if err != nil {
		t.Fatalf("failed reading repo config: %v", err)
	}
	return config
}

func (tr *testResult) getTestLogin() login {
	urlOpener := func(url string) error {
		tr.openedURL = url
		tr.stepCh <- struct{}{}
		return nil
	}
	return login{
		ui:       ui.Default(),
		logger:   hclog.Default(),
		repoRoot: tr.repoRoot,
		openURL:  urlOpener,
		client:   &tr.client,
		promptEnableCaching: func() (bool, error) {
			return tr.shouldEnableCaching, nil
		},
	}
}

func newTest(t *testing.T, repoRoot turbopath.AbsolutePath, redirectedURL string) *testResult {
	stepCh := make(chan struct{}, 1)
	tr := &testResult{
		repoRoot: repoRoot,
		stepCh:   stepCh,
	}
	tr.client.team = &client.Team{
		ID:         "sso-team-id",
		Membership: client.Membership{Role: "OWNER"},
	}
	tr.client.cachingStatus = util.CachingStatusEnabled
	// When it's time, do the redirect
	go func() {
		<-tr.stepCh
		client := &http.Client{
			CheckRedirect: func(req *http.Request, via []*http.Request) error {
				return http.ErrUseLastResponse
			},
		}
		resp, err := client.Get(redirectedURL)
		if err != nil {
			tr.clientErr = err
		} else if resp != nil && resp.StatusCode != http.StatusFound {
			tr.clientErr = fmt.Errorf("invalid status %v", resp.StatusCode)
		}
	}()
	return tr
}

func Test_run(t *testing.T) {
	cf := getConfig(t)
	test := newTest(t, cf.Cwd, "http://127.0.0.1:9789/?token=my-token")
	login := test.getTestLogin()
	err := login.run(cf)
	if err != nil {
		t.Errorf("expected to succeed, got error %v", err)
	}
	if test.clientErr != nil {
		t.Errorf("test client had error %v", test.clientErr)
	}

	expectedURL := "login-url/turborepo/token?redirect_uri=http://127.0.0.1:9789"
	if test.openedURL != expectedURL {
		t.Errorf("openedURL got %v, want %v", test.openedURL, expectedURL)
	}

	if cf.UserConfig.Token() != "my-token" {
		t.Errorf("config token got %v, want my-token", cf.UserConfig.Token())
	}
	if test.client.setToken != "my-token" {
		t.Errorf("user client token got %v, want my-token", test.client.setToken)
	}
}

func Test_sso(t *testing.T) {
	redirectParams := make(url.Values)
	redirectParams.Add("token", "verification-token")
	redirectParams.Add("email", "test@example.com")
	cf := getConfig(t)
	test := newTest(t, cf.Cwd, "http://127.0.0.1:9789/?"+redirectParams.Encode())
	login := test.getTestLogin()
	err := login.loginSSO(cf, "my-team")
	if err != nil {
		t.Errorf("expected to succeed, got error %v", err)
	}
	if test.clientErr != nil {
		t.Errorf("test client had error %v", test.clientErr)
	}
	host, err := os.Hostname()
	if err != nil {
		t.Errorf("failed to get hostname %v", err)
	}
	expectedTokenName := fmt.Sprintf("Turbo CLI on %v via SAML/OIDC Single Sign-On", host)
	if test.client.createdSSOTokenName != expectedTokenName {
		t.Errorf("created sso token got %v want %v", test.client.createdSSOTokenName, expectedTokenName)
	}
	expectedToken := "actual-sso-token"
	if test.client.setToken != expectedToken {
		t.Errorf("user client token got %v, want %v", test.client.setToken, expectedToken)
	}

	if cf.UserConfig.Token() != expectedToken {
		t.Errorf("user config token got %v want %v", cf.UserConfig.Token(), expectedToken)
	}
	repoConfigWritten := test.repoConfigWritten(t)
	remoteConfig := repoConfigWritten.GetRemoteConfig(expectedToken)
	expectedTeamID := "sso-team-id"
	if remoteConfig.TeamID != expectedTeamID {
		t.Errorf("repo config team id got %v want %v", remoteConfig.TeamID, expectedTeamID)
	}
}

func Test_ssoCachingDisabledShouldEnable(t *testing.T) {
	redirectParams := make(url.Values)
	redirectParams.Add("token", "verification-token")
	redirectParams.Add("email", "test@example.com")
	cf := getConfig(t)
	test := newTest(t, cf.Cwd, "http://127.0.0.1:9789/?"+redirectParams.Encode())
	test.shouldEnableCaching = true
	test.client.cachingStatus = util.CachingStatusDisabled
	login := test.getTestLogin()
	err := login.loginSSO(cf, "my-team")
	// Handle URL Open
	<-test.stepCh
	if !errors.Is(err, errTryAfterEnable) {
		t.Errorf("loginSSO got %v, want %v", err, errTryAfterEnable)
	}
}

func Test_ssoCachingDisabledDontEnable(t *testing.T) {
	redirectParams := make(url.Values)
	redirectParams.Add("token", "verification-token")
	redirectParams.Add("email", "test@example.com")
	cf := getConfig(t)
	test := newTest(t, cf.Cwd, "http://127.0.0.1:9789/?"+redirectParams.Encode())
	test.shouldEnableCaching = false
	test.client.cachingStatus = util.CachingStatusDisabled
	login := test.getTestLogin()
	err := login.loginSSO(cf, "my-team")
	if !errors.Is(err, errNeedCachingEnabled) {
		t.Errorf("loginSSO got %v, want %v", err, errNeedCachingEnabled)
	}
}
