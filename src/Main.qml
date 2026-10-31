pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import QtQuick.Shapes

ApplicationWindow {
    id: root

    // The Rust side. rowsJson: [{ name, code, secret, valid, expires (unix ms), period (s) }]
    // error: "" or why the last add/addUri/remove failed. Slots: refresh(), add(name, key), addUri(uri, name), remove(name)
    required property var backend

    property string undoName
    property string undoSecret

    function copy(text, what) {
        clipboard.text = text;
        clipboard.selectAll();
        clipboard.copy();
        toast.show(qsTr("Copied %1").arg(what));
    }

    function remove(name, secret) {
        backend.remove(name);
        if (backend.error) {
            toast.show(backend.error);
            return;
        }
        undoName = name;
        undoSecret = secret;
        backend.refresh();
        toast.show(qsTr("Deleted %1").arg(name), qsTr("Undo"));
    }

    function added(name) {
        prompt.error = backend.error;
        if (prompt.error)
            return;
        prompt.clear();
        prompt.close();
        backend.refresh();
        toast.show(qsTr("Added %1").arg(name));
    }

    visible: true
    width: 440
    height: 700
    minimumWidth: 340
    minimumHeight: 580
    title: "wauther"
    color: theme.surface

    Component.onCompleted: backend.refresh()

    // Material 3 dark scheme, blue seed. "on" colours are named *Text: QML reads onFoo as a signal handler.
    QtObject {
        id: theme

        readonly property color surface: "#101418"
        readonly property color surfaceContainer: "#1D2024"
        readonly property color surfaceContainerHigh: "#272A2F"
        readonly property color surfaceContainerHighest: "#32353A"
        readonly property color surfaceText: "#E0E2E8"
        readonly property color surfaceVariantText: "#C3C7CF"
        readonly property color outline: "#8D9199"
        readonly property color outlineVariant: "#43474E"
        readonly property color primary: "#9FCAFF"
        readonly property color primaryText: "#003258"
        readonly property color primaryContainer: "#00497D"
        readonly property color primaryContainerText: "#D1E4FF"
        readonly property color secondaryContainer: "#3B4858"
        readonly property color tertiaryContainer: "#523F5F"
        readonly property color error: "#FFB4AB"
        readonly property color errorContainer: "#93000A"
        readonly property color errorContainerText: "#FFDAD6"
        readonly property color inverseSurface: "#E0E2E8"
        readonly property color inverseText: "#2E3135"
    }

    Connections {
        function onRowsJsonChanged() {
            list.sync(root.backend.rowsJson);
        }

        target: root.backend
    }

    TextEdit {
        id: clipboard
        visible: false
    }

    Header {
        id: header
        width: parent.width
        anchors {
            top: parent.top
            topMargin: 28
        }
    }

    KeyList {
        id: list

        enabled: !prompt.opened && !menu.opened
        anchors {
            top: header.bottom
            topMargin: 12
            bottom: parent.bottom
            bottomMargin: 112
            left: parent.left
            leftMargin: 16
            right: parent.right
            rightMargin: 16
        }

        onRefreshNeeded: root.backend.refresh()
        onCopyRequested: (code, name) => root.copy(code, qsTr("code of %1").arg(name))
        onMenuRequested: (at, name, code, secret) => menu.openAt(at, name, code, secret)
    }

    Column {
        visible: list.count === 0
        spacing: 8
        anchors {
            horizontalCenter: parent.horizontalCenter
            top: header.bottom
            topMargin: 64
        }

        Text {
            anchors.horizontalCenter: parent.horizontalCenter
            text: qsTr("No keys yet")
            color: theme.surfaceVariantText
            font.pixelSize: 18
        }

        Text {
            anchors.horizontalCenter: parent.horizontalCenter
            text: qsTr("Press + to add one")
            color: theme.outline
            font.pixelSize: 14
        }
    }

    Fab {
        visible: prompt.progress < 0.01
        anchors {
            right: parent.right
            bottom: parent.bottom
        }

        onClicked: prompt.open()
    }

    Toast {
        id: toast

        anchors {
            left: parent.left
            leftMargin: 16
            bottom: parent.bottom
            bottomMargin: 24
        }

        onActionTriggered: {
            root.backend.add(root.undoName, root.undoSecret);
            root.backend.refresh();
        }
    }

    RadialMenu {
        id: menu

        anchors.fill: parent
        z: 1

        onCopyCode: root.copy(code, qsTr("code of %1").arg(name))
        onCopyKey: root.copy(secret, qsTr("key of %1").arg(name))
        onRemove: root.remove(name, secret)
    }

    Prompt {
        id: prompt

        anchors.fill: parent
        z: 2

        onAddManual: (name, key) => {
            root.backend.add(name, key);
            root.added(name.trim());
        }
        onAddUri: (uri, name) => {
            root.backend.addUri(uri, name);
            root.added(name || qsTr("key"));
        }
    }

    // ------------------------------------------------------------ shapes

    // A filler that turns a right angle between a container and an edge into a curve.
    // The box has the container's corner at: bottom-left (default), top-left (isTop), and the mirror of both (mirrored).
    component Concave: Shape {
        id: concave

        property int radius: 14
        property color color: "#1e1e2d"
        property bool isTop: false
        property bool mirrored: false

        implicitWidth: radius
        implicitHeight: radius
        preferredRendererType: Shape.CurveRenderer
        transform: Scale {
            xScale: concave.mirrored ? -1 : 1
            yScale: concave.isTop ? -1 : 1
            origin.x: concave.radius / 2
            origin.y: concave.radius / 2
        }

        ShapePath {
            fillColor: concave.color
            strokeColor: "transparent"
            startX: 0
            startY: 0

            PathLine {
                x: 0
                y: concave.radius
            }
            PathLine {
                x: concave.radius
                y: concave.radius
            }
            PathArc {
                x: 0
                y: 0
                radiusX: concave.radius
                radiusY: concave.radius
            }
        }
    }

    // A disc, filled clockwise from 12 o'clock over `fraction` of the circle.
    component Pie: Item {
        id: pie

        property real fraction
        property color color

        readonly property real r: width / 2

        implicitWidth: 28
        implicitHeight: 28

        Rectangle {
            anchors.fill: parent
            radius: pie.r
            color: theme.secondaryContainer
        }

        Shape {
            anchors.fill: parent
            visible: pie.fraction > 0.001
            preferredRendererType: Shape.CurveRenderer

            ShapePath {
                fillColor: pie.color
                strokeColor: "transparent"
                startX: pie.r
                startY: pie.r

                PathLine {
                    x: pie.r
                    y: 0
                }
                PathAngleArc {
                    centerX: pie.r
                    centerY: pie.r
                    radiusX: pie.r
                    radiusY: pie.r
                    startAngle: -90
                    sweepAngle: Math.min(359.9, 360 * pie.fraction)
                }
            }
        }
    }

    // ------------------------------------------------------------- icons

    component CopyIcon: Item {
        property color color
        property color fill

        implicitWidth: 20
        implicitHeight: 20

        Rectangle {
            width: 12
            height: 14
            radius: 2
            color: "transparent"
            border {
                width: 2
                color: parent.color
            }
        }
        Rectangle {
            x: 7
            y: 5
            width: 12
            height: 14
            radius: 2
            color: parent.fill
            border {
                width: 2
                color: parent.color
            }
        }
    }

    component KeyIcon: Item {
        property color color

        implicitWidth: 22
        implicitHeight: 16

        Rectangle {
            y: 1
            width: 10
            height: 10
            radius: 5
            color: "transparent"
            border {
                width: 2.5
                color: parent.color
            }
        }
        Rectangle {
            x: 10
            y: 4.5
            width: 12
            height: 2.5
            color: parent.color
        }
        Rectangle {
            x: 17
            y: 7
            width: 2.5
            height: 4
            color: parent.color
        }
    }

    component TrashIcon: Item {
        property color color

        implicitWidth: 18
        implicitHeight: 20

        Rectangle {
            x: 6
            width: 6
            height: 2.5
            color: parent.color
        }
        Rectangle {
            y: 3
            width: 18
            height: 2.5
            color: parent.color
        }
        Rectangle {
            x: 2
            y: 6
            width: 14
            height: 14
            bottomLeftRadius: 3
            bottomRightRadius: 3
            color: "transparent"
            border {
                width: 2.5
                color: parent.color
            }
        }
    }

    // ------------------------------------------------------------ header

    // A white W on a purple badge, then the rest of the name.
    component Brand: Row {
        spacing: 6

        Rectangle {
            id: badge

            width: 48
            height: 48
            radius: 14
            color: theme.tertiaryContainer

            Text {
                anchors.centerIn: parent
                text: "W"
                color: "white"
                font {
                    pixelSize: 32
                    bold: true
                }
            }

            // Tungsten's atomic number, like a periodic table cell.
            Text {
                text: "74"
                color: "#b3ffffff"
                font.pixelSize: 10
                anchors {
                    left: parent.left
                    bottom: parent.bottom
                    margins: 6
                }
            }
        }

        Text {
            anchors.verticalCenter: badge.verticalCenter
            text: "Auther"
            color: theme.surfaceText
            font {
                pixelSize: 34
                bold: true
            }
        }
    }

    // The brand, and three dots: still until hovered, then they blink one after another.
    component Header: Column {
        id: header

        spacing: 22

        Brand {
            anchors.horizontalCenter: parent.horizontalCenter
        }

        Item {
            width: parent.width
            height: 40

            HoverHandler {
                id: hover
            }

            Row {
                anchors.centerIn: parent
                spacing: 12

                Repeater {
                    model: 3

                    Rectangle {
                        id: dot

                        required property int index
                        property real lit: 0

                        width: 14
                        height: 14
                        radius: 7
                        color: Qt.tint(theme.outlineVariant, Qt.alpha(theme.primary, lit))

                        SequentialAnimation {
                            running: hover.hovered && header.visible
                            loops: Animation.Infinite
                            alwaysRunToEnd: true

                            PauseAnimation {
                                duration: dot.index * 480
                            }
                            NumberAnimation {
                                target: dot
                                property: "lit"
                                to: 1
                                duration: 600
                                easing.type: Easing.InOutSine
                            }
                            NumberAnimation {
                                target: dot
                                property: "lit"
                                to: 0
                                duration: 600
                                easing.type: Easing.InOutSine
                            }
                            PauseAnimation {
                                duration: 1200 - dot.index * 480
                            }
                        }
                    }
                }
            }
        }
    }

    // -------------------------------------------------------------- keys

    // rows: [{ name, code, secret, valid, expires (unix ms), period (s) }]
    component KeyList: ListView {
        id: list

        property real clock
        property real soonest: Infinity     // when the first code runs out

        signal refreshNeeded
        signal copyRequested(string code, string name)
        signal menuRequested(point at, string name, string code, string secret)

        function sync(json) {
            const rows = JSON.parse(json);
            if (rows.length === keys.count && rows.every((row, i) => row.name === keys.get(i).name)) {
                rows.forEach((row, i) => keys.set(i, row));
            } else {
                keys.clear();
                rows.forEach(row => keys.append(row));
            }
            soonest = Math.min(...rows.map(row => row.expires));
        }

        spacing: 3
        clip: true
        model: ListModel {
            id: keys
        }

        delegate: KeyRow {
            width: ListView.view.width
            count: list.count
            clock: list.clock

            onCopyRequested: list.copyRequested(code, name)
            onMenuRequested: at => list.menuRequested(at, name, code, secret)
            onDeleteRequested: root.remove(name, secret)
        }

        // Drives the pies, and asks for new codes the moment one runs out.
        Timer {
            interval: 33
            repeat: true
            running: list.visible
            triggeredOnStart: true
            onTriggered: {
                list.clock = Date.now();
                if (list.clock >= list.soonest)
                    list.refreshNeeded();
            }
        }
    }

    // Three rectangles in a row: name, code, pie. Hovering doubles the pie's, which uncovers delete.
    // Only the outermost corners of the table are round.
    component KeyRow: Item {
        id: row

        required property int index
        required property int count
        required property string name
        required property string code
        required property string secret
        required property bool valid
        required property real expires
        required property real period
        required property real clock

        readonly property real secs: Math.max(0, (expires - clock) / 1000)
        readonly property bool low: valid && secs <= 5
        readonly property int outerTop: index === 0 ? 28 : 8
        readonly property int outerBottom: index === count - 1 ? 28 : 8
        readonly property color fill: hover.hovered ? theme.surfaceContainerHigh : theme.surfaceContainer

        signal copyRequested
        signal menuRequested(point at)
        signal deleteRequested

        height: 72

        HoverHandler {
            id: hover
            cursorShape: Qt.PointingHandCursor
        }

        TapHandler {
            enabled: row.valid
            onTapped: row.copyRequested()
        }

        TapHandler {
            acceptedButtons: Qt.RightButton
            onTapped: eventPoint => row.menuRequested(eventPoint.scenePosition)
        }

        RowLayout {
            anchors.fill: parent
            spacing: 3

            Rectangle {
                Layout.fillWidth: true
                Layout.preferredWidth: 5
                Layout.fillHeight: true
                topLeftRadius: row.outerTop
                bottomLeftRadius: row.outerBottom
                topRightRadius: 8
                bottomRightRadius: 8
                color: row.fill

                Text {
                    text: row.name
                    elide: Text.ElideRight
                    verticalAlignment: Text.AlignVCenter
                    color: theme.surfaceText
                    font.pixelSize: 17
                    anchors {
                        fill: parent
                        leftMargin: 20
                        rightMargin: 12
                    }
                }
            }

            Rectangle {
                Layout.fillWidth: true
                Layout.preferredWidth: 6
                Layout.fillHeight: true
                radius: 8
                color: row.fill

                Text {
                    anchors.centerIn: parent
                    text: row.valid ? row.code.slice(0, row.code.length / 2) + " " + row.code.slice(row.code.length / 2) : qsTr("invalid key")
                    color: row.low || !row.valid ? theme.error : theme.primary
                    font {
                        family: "monospace"
                        pixelSize: row.valid ? 30 : 18
                        bold: true
                    }
                }
            }

            Rectangle {
                id: tail

                Layout.preferredWidth: hover.hovered ? 144 : 84
                Layout.fillHeight: true
                topLeftRadius: 8
                bottomLeftRadius: 8
                topRightRadius: row.outerTop
                bottomRightRadius: row.outerBottom
                color: row.fill

                Behavior on Layout.preferredWidth {
                    NumberAnimation {
                        duration: 180
                        easing.type: Easing.OutCubic
                    }
                }

                Pie {
                    fraction: row.valid ? Math.min(1, row.secs / row.period) : 0
                    color: row.low ? theme.error : theme.primary
                    anchors {
                        left: parent.left
                        leftMargin: 28
                        verticalCenter: parent.verticalCenter
                    }
                }

                // Joined to the rest of the tail by two concave curves.
                Rectangle {
                    id: del

                    width: Math.max(0, tail.width - 84)
                    visible: width > 1
                    topRightRadius: tail.topRightRadius
                    bottomRightRadius: tail.bottomRightRadius
                    color: theme.errorContainer
                    anchors {
                        top: parent.top
                        bottom: parent.bottom
                        right: parent.right
                    }

                    Concave {
                        x: -radius
                        radius: Math.min(16, Math.round(del.width / 4))
                        color: del.color
                        isTop: true
                        mirrored: true
                    }

                    Concave {
                        x: -radius
                        y: del.height - radius
                        radius: Math.min(16, Math.round(del.width / 4))
                        color: del.color
                        mirrored: true
                    }

                    TrashIcon {
                        anchors.centerIn: parent
                        visible: del.width > 40
                        color: theme.errorContainerText
                    }

                    TapHandler {
                        onTapped: row.deleteRequested()
                    }
                }
            }
        }
    }

    // Right click on a key: copy its code (left), copy its key (top), delete it (right).
    component RadialMenu: Item {
        id: menu

        property bool opened
        property point at
        property string name
        property string code
        property string secret

        readonly property real cx: Math.min(Math.max(at.x, 92), width - 92)
        readonly property real cy: Math.min(Math.max(at.y, 92), height - 40)

        signal copyCode
        signal copyKey
        signal remove

        function openAt(scene, rowName, rowCode, rowSecret) {
            at = scene;
            name = rowName;
            code = rowCode;
            secret = rowSecret;
            opened = true;
        }

        visible: opened

        TapHandler {
            acceptedButtons: Qt.AllButtons
            onTapped: menu.opened = false
        }

        component Action: Rectangle {
            id: action

            property bool danger
            property real dx
            property real dy
            readonly property color ink: danger ? theme.errorContainerText : theme.surfaceText

            signal clicked

            x: menu.cx + dx - width / 2
            y: menu.cy + dy - height / 2
            width: 52
            height: 52
            radius: 26
            scale: menu.opened ? 1 : 0.4
            color: Qt.tint(danger ? theme.errorContainer : theme.surfaceContainerHighest, Qt.alpha(ink, tap.hovered ? 0.12 : 0))

            Behavior on scale {
                NumberAnimation {
                    duration: 160
                    easing.type: Easing.OutBack
                }
            }

            HoverHandler {
                id: tap
                cursorShape: Qt.PointingHandCursor
            }

            TapHandler {
                onTapped: {
                    menu.opened = false;
                    action.clicked();
                }
            }
        }

        Action {
            dx: -62
            onClicked: menu.copyCode()

            CopyIcon {
                anchors.centerIn: parent
                color: parent.ink
                fill: parent.color
            }
        }

        Action {
            dy: -62
            onClicked: menu.copyKey()

            KeyIcon {
                anchors.centerIn: parent
                color: parent.ink
            }
        }

        Action {
            dx: 62
            danger: true
            onClicked: menu.remove()

            TrashIcon {
                anchors.centerIn: parent
                color: parent.ink
            }
        }
    }

    // ---------------------------------------------------- fab and toast

    // The big +, in the bottom-right corner, joined to the window edges by two concave curves.
    component Fab: Item {
        id: fab

        readonly property color fill: Qt.tint(theme.primaryContainer, Qt.alpha(theme.primaryContainerText, hover.hovered ? 0.12 : 0))

        signal clicked

        implicitWidth: 88
        implicitHeight: 88

        HoverHandler {
            id: hover
            cursorShape: Qt.PointingHandCursor
        }

        TapHandler {
            onTapped: fab.clicked()
        }

        Rectangle {
            anchors.fill: parent
            topLeftRadius: 28
            color: fab.fill
        }

        Concave {
            x: fab.width - radius
            y: -radius
            radius: 16
            color: fab.fill
            mirrored: true
        }

        Concave {
            x: -radius
            y: fab.height - radius
            radius: 16
            color: fab.fill
            mirrored: true
        }

        Repeater {
            model: [Qt.size(30, 6), Qt.size(6, 30)]

            Rectangle {
                required property size modelData

                anchors.centerIn: parent
                width: modelData.width
                height: modelData.height
                radius: 3
                color: theme.primaryContainerText
            }
        }
    }

    // A message, with an optional action (Undo).
    component Toast: Rectangle {
        id: toast

        property string message
        property string action

        signal actionTriggered

        function show(text, label = "") {
            message = text;
            action = label;
            hide.restart();
        }

        width: content.width + 32
        height: 48
        radius: 12
        visible: hide.running
        color: theme.inverseSurface

        Row {
            id: content

            anchors.centerIn: parent
            spacing: 16

            Text {
                anchors.verticalCenter: parent.verticalCenter
                text: toast.message
                color: theme.inverseText
                font.pixelSize: 14
            }

            Text {
                anchors.verticalCenter: parent.verticalCenter
                visible: toast.action !== ""
                text: toast.action
                color: theme.primaryContainer
                font {
                    pixelSize: 14
                    bold: true
                }

                TapHandler {
                    onTapped: {
                        toast.actionTriggered();
                        hide.stop();
                    }
                }
            }
        }

        Timer {
            id: hide
            interval: 4000
        }
    }

    // ------------------------------------------------------------ prompt

    // A text box with a label above it. With `rise`, typed letters rise into place from below.
    component Field: ColumnLayout {
        id: field

        property string label
        property string placeholder
        property bool rise
        property alias text: input.text

        signal accepted

        function focusInput() {
            input.forceActiveFocus();
        }

        function clear() {
            input.clear();
        }

        spacing: 6

        Text {
            text: field.label
            color: theme.surfaceVariantText
            font.pixelSize: 13
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 56
            radius: 20
            color: theme.surfaceContainerHighest
            border {
                width: input.activeFocus ? 2 : 0
                color: theme.primary
            }

            // Does the editing. In `rise` mode it is invisible and the letters below stand in for its text.
            TextInput {
                id: input

                verticalAlignment: TextInput.AlignVCenter
                clip: true
                maximumLength: field.rise ? 32 : 32767
                font.pixelSize: 16
                color: field.rise ? "transparent" : theme.surfaceText
                selectionColor: field.rise ? "transparent" : theme.primaryContainer
                onAccepted: field.accepted()
                anchors {
                    fill: parent
                    leftMargin: 20
                    rightMargin: 20
                }
            }

            Text {
                visible: input.text.length === 0
                text: field.placeholder
                color: theme.outline
                font.pixelSize: 16
                anchors {
                    left: parent.left
                    leftMargin: 20
                    verticalCenter: parent.verticalCenter
                }
            }

            Row {
                id: letters

                visible: field.rise
                anchors {
                    left: parent.left
                    leftMargin: 20
                    verticalCenter: parent.verticalCenter
                }

                Repeater {
                    id: chars

                    model: field.rise ? input.text.length : 0

                    Text {
                        id: letter

                        required property int index
                        property real lift: 1

                        text: input.text.charAt(index)
                        color: theme.surfaceText
                        font.pixelSize: 16
                        opacity: 1 - lift
                        transform: Translate {
                            y: letter.lift * 16
                        }

                        NumberAnimation on lift {
                            from: 1
                            to: 0
                            duration: 260
                            easing.type: Easing.OutCubic
                        }
                    }
                }
            }

            Rectangle {
                width: 2
                height: 22
                radius: 1
                visible: field.rise && input.activeFocus
                color: theme.primary
                anchors.verticalCenter: parent.verticalCenter
                x: {
                    const item = chars.itemAt(input.cursorPosition);
                    return letters.x + (item ? item.x : letters.width);
                }

                SequentialAnimation on opacity {
                    running: field.rise && input.activeFocus
                    loops: Animation.Infinite

                    NumberAnimation {
                        to: 0
                        duration: 500
                    }
                    NumberAnimation {
                        to: 1
                        duration: 500
                    }
                }
            }
        }
    }

    // Two modes, one at each end of a line. A marker on the line moves to the current one.
    component ModeTabs: Item {
        id: tabs

        property int current

        implicitHeight: 48

        component Label: Text {
            id: label

            required property int mode

            color: tabs.current === mode ? theme.primary : theme.surfaceVariantText
            font.pixelSize: 16
            anchors.top: parent.top

            TapHandler {
                onTapped: tabs.current = label.mode
            }
        }

        Label {
            id: manual
            mode: 0
            text: qsTr("Manually")
        }

        Label {
            mode: 1
            text: qsTr("From URI")
            anchors.right: parent.right
        }

        Rectangle {
            width: parent.width
            height: 1
            color: theme.outlineVariant
            anchors.bottom: parent.bottom
        }

        Shape {
            id: marker

            x: (tabs.current === 0 ? manual.width / 2 : tabs.width - manual.width / 2) - 40
            width: 80
            height: 7
            anchors.bottom: parent.bottom
            preferredRendererType: Shape.CurveRenderer

            Behavior on x {
                NumberAnimation {
                    duration: 250
                    easing.type: Easing.OutCubic
                }
            }

            ShapePath {
                fillColor: theme.primary
                strokeColor: "transparent"
                startX: 0
                startY: 7

                PathLine {
                    x: 10
                    y: 0
                }
                PathLine {
                    x: 70
                    y: 0
                }
                PathLine {
                    x: 80
                    y: 7
                }
            }
        }
    }

    // Slides down from the top. It only emits signals; the parent answers by setting `error`, or with clear() and close().
    component Prompt: Item {
        id: prompt

        property bool opened
        property real progress: opened ? 1 : 0
        property string error

        signal addManual(string name, string key)
        signal addUri(string uri, string name)

        function open() {
            error = "";
            opened = true;
            Qt.callLater(focusFirst);
        }

        function close() {
            opened = false;
        }

        function clear() {
            [nameField, keyField, uriField, uriNameField].forEach(field => field.clear());
        }

        function focusFirst() {
            (tabs.current === 0 ? nameField : uriField).focusInput();
        }

        function submit() {
            error = "";
            if (tabs.current === 0)
                addManual(nameField.text, keyField.text.trim());
            else
                addUri(uriField.text.trim(), uriNameField.text.trim());
        }

        visible: progress > 0.001

        Behavior on progress {
            NumberAnimation {
                duration: 300
                easing.type: Easing.OutCubic
            }
        }

        Shortcut {
            sequence: "Esc"
            enabled: prompt.opened
            onActivated: prompt.close()
        }

        Rectangle {
            anchors.fill: parent
            color: Qt.rgba(0, 0, 0, 0.5 * prompt.progress)

            TapHandler {
                onTapped: prompt.close()
            }
        }

        Rectangle {
            id: panel

            width: parent.width
            height: Math.min(parent.height, form.implicitHeight + 64)
            y: -height * (1 - prompt.progress)
            color: theme.surfaceContainerHigh

            // Keeps clicks on the panel from reaching the scrim.
            MouseArea {
                anchors.fill: parent
            }

            // The curves joining the panel's bottom edge to the window's sides.
            Concave {
                y: panel.height
                radius: 28
                color: panel.color
                isTop: true
            }

            Concave {
                x: panel.width - radius
                y: panel.height
                radius: 28
                color: panel.color
                isTop: true
                mirrored: true
            }

            // Close: a corner of its own, joined to the panel's top and right edges by concave curves.
            Item {
                id: corner

                width: 64
                height: 64
                anchors {
                    top: parent.top
                    right: parent.right
                }

                HoverHandler {
                    id: cornerHover
                    cursorShape: Qt.PointingHandCursor
                }

                TapHandler {
                    onTapped: prompt.close()
                }

                Rectangle {
                    anchors.fill: parent
                    bottomLeftRadius: 24
                    color: Qt.tint(theme.surfaceContainerHighest, Qt.alpha(theme.surfaceText, cornerHover.hovered ? 0.12 : 0))
                }

                Concave {
                    x: -radius
                    radius: 16
                    color: theme.surfaceContainerHighest
                    isTop: true
                    mirrored: true
                }

                Concave {
                    x: corner.width - radius
                    y: corner.height
                    radius: 16
                    color: theme.surfaceContainerHighest
                    isTop: true
                    mirrored: true
                }

                Repeater {
                    model: [45, -45]

                    Rectangle {
                        required property int modelData

                        anchors.centerIn: parent
                        width: 22
                        height: 2.5
                        radius: 1.25
                        rotation: modelData
                        color: theme.surfaceText
                    }
                }
            }

            ColumnLayout {
                id: form

                spacing: 20
                anchors {
                    top: parent.top
                    topMargin: 28
                    left: parent.left
                    leftMargin: 28
                    right: parent.right
                    rightMargin: 28
                }

                Brand {
                    Layout.alignment: Qt.AlignHCenter
                }

                Text {
                    Layout.alignment: Qt.AlignHCenter
                    text: qsTr("Add a key")
                    color: theme.surfaceText
                    font {
                        pixelSize: 30
                        letterSpacing: 2
                    }
                }

                ModeTabs {
                    id: tabs

                    Layout.fillWidth: true

                    onCurrentChanged: {
                        prompt.error = "";
                        Qt.callLater(prompt.focusFirst);
                    }
                }

                StackLayout {
                    Layout.fillWidth: true
                    currentIndex: tabs.current

                    ColumnLayout {
                        spacing: 16

                        Field {
                            id: nameField

                            Layout.fillWidth: true
                            label: qsTr("Name")
                            placeholder: qsTr("e.g. GitHub")
                            rise: true

                            onAccepted: prompt.submit()
                        }

                        Field {
                            id: keyField

                            Layout.fillWidth: true
                            label: qsTr("Key")
                            placeholder: qsTr("Base32 secret")

                            onAccepted: prompt.submit()
                        }
                    }

                    ColumnLayout {
                        spacing: 16

                        Field {
                            id: uriField

                            Layout.fillWidth: true
                            label: qsTr("URI")
                            placeholder: "otpauth://totp/…"

                            onAccepted: prompt.submit()
                        }

                        Field {
                            id: uriNameField

                            Layout.fillWidth: true
                            label: qsTr("Name")
                            placeholder: qsTr("Taken from the URI if empty")
                            rise: true

                            onAccepted: prompt.submit()
                        }
                    }
                }

                Text {
                    Layout.fillWidth: true
                    visible: prompt.error !== ""
                    text: prompt.error
                    wrapMode: Text.WordWrap
                    color: theme.error
                    font.pixelSize: 13
                }

                Button {
                    id: add

                    Layout.alignment: Qt.AlignRight
                    Layout.preferredWidth: 96
                    Layout.preferredHeight: 40
                    text: qsTr("Add")
                    onClicked: prompt.submit()

                    contentItem: Text {
                        text: add.text
                        color: theme.primaryText
                        horizontalAlignment: Text.AlignHCenter
                        verticalAlignment: Text.AlignVCenter
                        font {
                            pixelSize: 15
                            weight: Font.Medium
                        }
                    }

                    background: Rectangle {
                        radius: 20
                        color: Qt.tint(theme.primary, Qt.alpha(theme.primaryText, add.hovered ? 0.12 : 0))
                    }
                }
            }
        }
    }
}
