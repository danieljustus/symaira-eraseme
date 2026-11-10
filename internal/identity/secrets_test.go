package identity

import (
	"sync"

	"github.com/zalando/go-keyring"
)

// FakeKeyring is an in-memory KeyringBackend for isolated testing.
type FakeKeyring struct {
	mu       sync.RWMutex
	store    map[string]string
	GetError error
	SetError error
	DelError error
}

// NewFakeKeyring creates a thread-safe in-memory fake keyring for testing.
func NewFakeKeyring() *FakeKeyring {
	return &FakeKeyring{
		store: make(map[string]string),
	}
}

func (f *FakeKeyring) Get(service, username string) (string, error) {
	f.mu.RLock()
	defer f.mu.RUnlock()
	if f.GetError != nil {
		return "", f.GetError
	}
	val, ok := f.store[service+":"+username]
	if !ok {
		return "", keyring.ErrNotFound
	}
	return val, nil
}

func (f *FakeKeyring) Set(service, username, password string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.SetError != nil {
		return f.SetError
	}
	f.store[service+":"+username] = password
	return nil
}

func (f *FakeKeyring) Delete(service, username string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.DelError != nil {
		return f.DelError
	}
	delete(f.store, service+":"+username)
	return nil
}
