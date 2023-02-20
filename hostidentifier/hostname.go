package hostidentifier

import (
	"os"

	log "github.com/sirupsen/logrus"
)

type Hostname struct{}

func (h Hostname) HostID() string {
	hostname, err := os.Hostname()
	if err != nil {
		log.Errorf("error fetching hostname for hostID")
		return ""
	}

	return hostname
}
