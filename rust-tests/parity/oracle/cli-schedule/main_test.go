package main

import "testing"

func TestFoldWindowsEscapedAndLiteralPaths(t *testing.T) {
	root := `C:\isolated\root`
	caseRoot := root + `\case-install`
	value := `{"path":"C:\\isolated\\root\\case-install\\data","other":"D:\\keep"}` + "\n" + caseRoot + `\data`
	want := `{"path":"<CASE>\\data","other":"D:\\keep"}` + "\n" + `<CASE>\data`
	if got := fold(value, root, caseRoot); got != want {
		t.Fatalf("fold escaped/literal paths:\n got %q\nwant %q", got, want)
	}
}
