// Package lockfile provides the lockfile interface and implementations for the various package managers
package lockfile

import "io"

// Lockfile Interface for general operations that work accross all lockfiles
type Lockfile interface {
	// ResolvePackage Given a package and version returns the key, resolved version, and if it was found
	ResolvePackage(name string, version string) (string, string, bool)
	// AllDependencies Given a lockfile key return all (dev/optional/peer) dependencies of that package
	AllDependencies(key string) (map[string]string, bool)
	// Subgraph Given a list of lockfile keys returns a Lockfile based off the original one that only contains the packages given
	Subgraph(packages []string) (Lockfile, error)
	// Encode encode the lockfile representation and write it to the given writer
	Encode(w io.Writer) error
}
