// The "add key" prompt. Slides down from the top and covers half the window.
// It never touches the backend: it emits a signal and the parent answers with
// close() / clear() on success or showError() on failure.
import QtQuick
import QtQuick.Layouts

Item {
    id: root

    required property var theme

    property bool opened: false
    property real progress: opened ? 1 : 0
    property int tab: 0                 // 0 = add manually, 1 = from URI
    property string error: ""

    signal addManual(string name, string key)
    signal addUri(string uri, string name)

    anchors.fill: parent
    z: 10
    visible: progress > 0.001

    Behavior on progress {
        NumberAnimation { duration: 300; easing.type: Easing.OutCubic }
    }

    function open() {
        error = "";
        opened = true;
    }

    function close() {
        opened = false;
        root.forceActiveFocus();
    }

    function clear() {
        nameField.text = "";
        keyField.text = "";
        uriField.text = "";
        uriNameField.text = "";
        error = "";
    }

    function showError(message) {
        error = message;
    }

    function setTab(index) {
        tab = index;
        error = "";
        Qt.callLater(focusFirst);
    }

    function focusFirst() {
        (tab === 0 ? nameField : uriField).focusInput();
    }

    function submit() {
        error = "";
        if (tab === 0)
            addManual(nameField.text, keyField.text.trim());
        else
            addUri(uriField.text.trim(), uriNameField.text.trim());
    }

    onOpenedChanged: if (opened) Qt.callLater(focusFirst)

    Shortcut {
        sequence: "Esc"
        enabled: root.opened
        onActivated: root.close()
    }

    // A filled text field: label on top, text under it, indicator along the bottom.
    component Field: Rectangle {
        id: field

        property string label
        property string hint
        property alias text: input.text

        signal accepted

        function focusInput() {
            input.forceActiveFocus();
        }

        Layout.fillWidth: true
        Layout.preferredHeight: 56
        color: root.theme.surfaceContainerHighest
        topLeftRadius: 12
        topRightRadius: 12
        bottomLeftRadius: 4
        bottomRightRadius: 4

        Rectangle {
            anchors.bottom: parent.bottom
            width: parent.width
            height: input.activeFocus ? 2 : 1
            color: input.activeFocus ? root.theme.primary : root.theme.onSurfaceVariant
        }

        Text {
            x: 16
            y: 8
            text: field.label
            font.pixelSize: 12
            color: input.activeFocus ? root.theme.primary : root.theme.onSurfaceVariant
        }

        Text {
            x: 16
            y: 26
            visible: input.text.length === 0
            text: field.hint
            font.pixelSize: 16
            color: root.theme.outline
        }

        TextInput {
            id: input
            x: 16
            y: 26
            width: parent.width - 32
            clip: true
            activeFocusOnTab: true
            font.pixelSize: 16
            color: root.theme.onSurface
            selectionColor: root.theme.primaryContainer
            selectedTextColor: root.theme.onPrimaryContainer
            onAccepted: field.accepted()
            onTextEdited: root.error = ""
        }
    }

    component PillButton: Rectangle {
        id: pill

        property string label
        property bool filled: false

        signal clicked

        implicitWidth: 96
        implicitHeight: 40
        radius: 20
        color: filled ? root.theme.primary : "transparent"

        Rectangle {
            anchors.fill: parent
            radius: parent.radius
            color: pill.filled ? root.theme.onPrimary : root.theme.primary
            opacity: hover.hovered ? 0.12 : 0
            Behavior on opacity { NumberAnimation { duration: 120 } }
        }

        Text {
            anchors.centerIn: parent
            text: pill.label
            font.pixelSize: 15
            font.weight: Font.Medium
            color: pill.filled ? root.theme.onPrimary : root.theme.primary
        }

        HoverHandler { id: hover }
        MouseArea {
            anchors.fill: parent
            cursorShape: Qt.PointingHandCursor
            onClicked: pill.clicked()
        }
    }

    Rectangle {
        anchors.fill: parent
        color: "black"
        opacity: 0.5 * root.progress

        MouseArea {
            anchors.fill: parent
            onClicked: root.close()
        }
    }

    Rectangle {
        id: panel

        width: parent.width
        height: Math.min(parent.height, Math.max(parent.height * 0.5, 320))
        y: -height * (1 - root.progress)
        color: root.theme.surfaceContainerHigh

        // Swallows clicks, so they don't reach the scrim.
        MouseArea { anchors.fill: parent }

        // The curves that join the panel to the window's side edges.
        Concave {
            x: 0
            y: panel.height
            radius: 28
            color: panel.color
            isTop: true
            mirrored: false
        }
        Concave {
            x: panel.width - 28
            y: panel.height
            radius: 28
            color: panel.color
            isTop: true
            mirrored: true
        }

        ColumnLayout {
            anchors.fill: parent
            anchors.leftMargin: 24
            anchors.rightMargin: 24
            anchors.topMargin: 24
            anchors.bottomMargin: 20
            spacing: 12

            // Two options: add by hand, or from an otpauth:// URI.
            Rectangle {
                id: segmented

                Layout.fillWidth: true
                Layout.preferredHeight: 40
                Layout.bottomMargin: 4
                radius: 20
                color: "transparent"
                border.color: root.theme.outline
                border.width: 1

                Rectangle {
                    x: root.tab === 0 ? 2 : segmented.width / 2
                    y: 2
                    width: segmented.width / 2 - 2
                    height: segmented.height - 4
                    radius: 18
                    color: root.theme.secondaryContainer
                    Behavior on x { NumberAnimation { duration: 200; easing.type: Easing.OutCubic } }
                }

                Row {
                    anchors.fill: parent

                    Repeater {
                        model: ["Add manually", "From URI"]

                        Item {
                            id: segment

                            required property int index
                            required property string modelData

                            width: segmented.width / 2
                            height: segmented.height

                            Text {
                                anchors.centerIn: parent
                                text: segment.modelData
                                font.pixelSize: 15
                                font.weight: Font.Medium
                                color: root.tab === segment.index
                                    ? root.theme.onSecondaryContainer
                                    : root.theme.onSurfaceVariant
                            }
                            MouseArea {
                                anchors.fill: parent
                                cursorShape: Qt.PointingHandCursor
                                onClicked: root.setTab(segment.index)
                            }
                        }
                    }
                }
            }

            StackLayout {
                Layout.fillWidth: true
                Layout.preferredHeight: 124
                currentIndex: root.tab

                ColumnLayout {
                    spacing: 12
                    Field {
                        id: nameField
                        label: "Key name"
                        hint: "e.g. GitHub"
                        onAccepted: root.submit()
                    }
                    Field {
                        id: keyField
                        label: "Key"
                        hint: "Base32 secret"
                        onAccepted: root.submit()
                    }
                }

                ColumnLayout {
                    spacing: 12
                    Field {
                        id: uriField
                        label: "URI"
                        hint: "otpauth://totp/…"
                        onAccepted: root.submit()
                    }
                    Field {
                        id: uriNameField
                        label: "Key name"
                        hint: "Taken from the URI if empty"
                        onAccepted: root.submit()
                    }
                }
            }

            Text {
                Layout.fillWidth: true
                visible: root.error !== ""
                text: root.error
                wrapMode: Text.WordWrap
                font.pixelSize: 13
                color: root.theme.error
            }

            Item { Layout.fillHeight: true }

            RowLayout {
                Layout.alignment: Qt.AlignRight
                spacing: 8

                PillButton {
                    label: "Cancel"
                    onClicked: root.close()
                }
                PillButton {
                    label: "Add"
                    filled: true
                    onClicked: root.submit()
                }
            }
        }
    }
}
