package hostidentifier

type Fake string

func (f Fake) HostID() string {
	return string(f)
}
