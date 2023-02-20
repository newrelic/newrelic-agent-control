package hostidentifier

type IDer interface {
	HostID() string
}

type List struct {
	iders []IDer
}

func ListWith(iders ...IDer) List {
	return List{}.With(iders...)
}

func (l List) With(iders ...IDer) List {
	return List{
		iders: append(l.iders, iders...),
	}
}

func (l List) HostID() string {
	for _, ider := range l.iders {
		id := ider.HostID()
		if id != "" {
			return id
		}
	}

	return ""
}
