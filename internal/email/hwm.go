package email

import (
	"context"
	"fmt"
	"sync"
)

type stagedHWMRecord struct {
	host        string
	folder      string
	uidValidity uint32
	lastUID     uint32
}

// StagingHWMStore keeps HWM updates in memory until Commit. It is used by
// InboxService to make reply persistence happen before durable HWM advancement.
type StagingHWMStore struct {
	underlying HWMStore
	mu         sync.Mutex
	staged     []stagedHWMRecord
}

func NewStagingHWMStore(underlying HWMStore) *StagingHWMStore {
	return &StagingHWMStore{underlying: underlying}
}

func (s *StagingHWMStore) Get(ctx context.Context, host, folder string) (*uint32, *uint32, error) {
	s.mu.Lock()
	for i := len(s.staged) - 1; i >= 0; i-- {
		if s.staged[i].host == host && s.staged[i].folder == folder {
			v, u := s.staged[i].uidValidity, s.staged[i].lastUID
			s.mu.Unlock()
			return &v, &u, nil
		}
	}
	underlying := s.underlying
	s.mu.Unlock()
	if underlying == nil {
		return nil, nil, nil
	}
	return underlying.Get(ctx, host, folder)
}

func (s *StagingHWMStore) Set(_ context.Context, host, folder string, uidValidity, lastUID uint32) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.staged = append(s.staged, stagedHWMRecord{host: host, folder: folder, uidValidity: uidValidity, lastUID: lastUID})
	return nil
}

func (s *StagingHWMStore) Commit(ctx context.Context) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.underlying == nil {
		s.staged = nil
		return nil
	}
	for _, rec := range s.staged {
		if err := s.underlying.Set(ctx, rec.host, rec.folder, rec.uidValidity, rec.lastUID); err != nil {
			return fmt.Errorf("email: commit high-water mark (%s/%s): %w", rec.host, rec.folder, err)
		}
	}
	s.staged = nil
	return nil
}
