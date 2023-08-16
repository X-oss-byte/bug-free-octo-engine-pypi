package filter

import (
	"errors"
	"path/filepath"
	"regexp"
	"strings"
)

type TargetSelector struct {
	includeDependencies bool
	matchDependencies   bool
	includeDependents   bool
	exclude             bool
	excludeSelf         bool
	followProdDepsOnly  bool
	parentDir           string
	namePattern         string
	diff                string
	raw                 string
}

func (ts *TargetSelector) IsValid() bool {
	return ts.diff != "" || ts.parentDir != "" || ts.namePattern != ""
}

var errCantMatchDependencies = errors.New("cannot use match dependencies without specifying either a directory or package")

var targetSelectorRegex = regexp.MustCompile(`^([^.](?:[^{}[\]]*[^{}[\].])?)?(\{[^}]+\})?((?:\.{3})?\[[^\]]+\])?$`)

// ParseTargetSelector is a function that returns pnpm compatible --filter command line flags
func ParseTargetSelector(rawSelector string, prefix string) (TargetSelector, error) {
	exclude := false
	firstChar := rawSelector[0]
	selector := rawSelector
	if firstChar == '!' {
		selector = selector[1:]
		exclude = true
	}
	excludeSelf := false
	includeDependencies := strings.HasSuffix(selector, "...")
	if includeDependencies {
		selector = selector[:len(selector)-3]
		if strings.HasSuffix(selector, "^") {
			excludeSelf = true
			selector = selector[:len(selector)-1]
		}
	}
	includeDependents := strings.HasPrefix(selector, "...")
	if includeDependents {
		selector = selector[3:]
		if strings.HasPrefix(selector, "^") {
			excludeSelf = true
			selector = selector[1:]
		}
	}

	matches := targetSelectorRegex.FindAllStringSubmatch(selector, -1)

	diff := ""
	parentDir := ""
	namePattern := ""

	if len(matches) == 0 {
		if isSelectorByLocation(selector) {
			return TargetSelector{
				diff:                diff,
				exclude:             exclude,
				excludeSelf:         false,
				includeDependencies: includeDependencies,
				includeDependents:   includeDependents,
				namePattern:         namePattern,
				parentDir:           filepath.Join(prefix, selector),
				raw:                 rawSelector,
			}, nil
		}
		return TargetSelector{
			diff:                diff,
			exclude:             exclude,
			excludeSelf:         excludeSelf,
			includeDependencies: includeDependencies,
			includeDependents:   includeDependents,
			namePattern:         selector,
			parentDir:           parentDir,
			raw:                 rawSelector,
		}, nil
	}

	preAddDepdencies := false
	if len(matches) > 0 && len(matches[0]) > 0 {
		if len(matches[0][1]) > 0 {
			namePattern = matches[0][1]
		}
		if len(matches[0][2]) > 0 {
			parentDir = matches[0][2]
			parentDir = filepath.Join(prefix, parentDir[1:len(parentDir)-1])
		}
		if len(matches[0][3]) > 0 {
			diff = matches[0][3]
			if strings.HasPrefix(diff, "...") {
				if parentDir == "" && namePattern == "" {
					return TargetSelector{}, errCantMatchDependencies
				}
				preAddDepdencies = true
				diff = diff[3:]
			}
			// strip []
			diff = diff[1 : len(diff)-1]
		}
	}

	return TargetSelector{
		diff:                diff,
		exclude:             exclude,
		excludeSelf:         excludeSelf,
		includeDependencies: includeDependencies,
		matchDependencies:   preAddDepdencies,
		includeDependents:   includeDependents,
		namePattern:         namePattern,
		parentDir:           parentDir,
		raw:                 rawSelector,
	}, nil
}

// isSelectorByLocation returns true if the selector is by filesystem location
func isSelectorByLocation(rawSelector string) bool {
	if rawSelector[0:1] != "." {
		return false
	}

	// . or ./ or .\
	if len(rawSelector) == 1 || rawSelector[1:2] == "/" || rawSelector[1:2] == "\\" {
		return true
	}

	if rawSelector[1:2] != "." {
		return false
	}

	// .. or ../ or ..\
	return len(rawSelector) == 2 || rawSelector[2:3] == "/" || rawSelector[2:3] == "\\"
}
